//! The tray entry on macOS: an item in the menu bar's system status
//! area (`NSStatusItem`), whose menu AppKit opens when the item is
//! clicked — Open Pane, Settings, Quit Pane, the platform's own labels.
//! Made and changed on the main thread, as the hotkey adapter's
//! registrations are (the window's thread, whose run loop the item
//! lives on); no permission is needed.
//!
//! The menu's items are given a target of Pane's own, a small Objective-C
//! class this module defines, whose action methods report the selection
//! through the [`SelectionSender`] the adapter was made with. The target
//! is kept by the adapter (the menu's items do not keep their target, so
//! nothing else may own it), and the item is removed from the status bar
//! when the adapter is dropped or Pane quits.
//!
//! The item's button carries Pane's name as its title — honest and
//! legible where an icon would be blank in a development build, which is
//! what the application icon would be there. The native validation
//! records how a run sees it.

use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{ClassType, DefinedClass, MainThreadMarker, define_class, msg_send, sel};
use objc2_app_kit::{NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSObjectProtocol, NSString};

use super::{SelectionSender, Tray, TrayAction, TrayError};

// Links AppKit, whose classes the adapter uses: the bindings crate links
// nothing (objc2 links only Foundation), and without this the classes'
// symbols would not resolve in Pane's own tests, which link no GPUI that
// links AppKit for them.
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

/// The adapter: the status bar, the target that owns the menu's
/// selections, and the item while it is in the menu bar. The mutex is
/// Pane's own: the trait is shared, while the item itself belongs to the
/// main thread, which every caller of the adapter is on (see the module
/// docs).
pub struct MacTray {
    state: Mutex<State>,
}

struct State {
    /// The system status bar, which owns the item's place.
    bar: Retained<NSStatusBar>,
    /// The menu items' target, which the adapter must keep alive for as
    /// long as any item can send to it.
    target: Retained<TrayTarget>,
    /// The item, while one is made; `None` while the entry is hidden and
    /// none was ever shown. A hidden item is kept — showing it again
    /// needs no new one — and removed from the bar when the adapter is
    /// dropped.
    item: Option<Retained<NSStatusItem>>,
}

// SAFETY: the AppKit objects `State` holds belong to the main thread,
// and the adapter is the only thing that ever touches them, always on
// it: `MacTray::start` refuses any other thread, and `set_visible`
// checks the marker before it makes the item. Moving the state to
// another thread — which is all `Send` does — never uses the objects,
// and the `Retained`s' own reference counting, which their drop
// releases, is thread-safe, so the adapter's one discipline carries the
// sharing the trait asks for.
unsafe impl Send for State {}

/// The menu items' target: one action method per menu item, each
/// reporting its selection. The class carries no AppKit behavior of its
/// own — it is a plain `NSObject` the items send to — and its only state
/// is the channel the selections go through.
struct TargetIvars {
    selections: SelectionSender,
}

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements, and
    // `TrayTarget` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[ivars = TargetIvars]
    struct TrayTarget;

    unsafe impl NSObjectProtocol for TrayTarget {}

    impl TrayTarget {
        #[unsafe(method(openPane:))]
        fn open_pane(&self, _item: &NSMenuItem) {
            self.ivars().selections.send(TrayAction::OpenPane);
        }
        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _item: &NSMenuItem) {
            self.ivars().selections.send(TrayAction::Settings);
        }
        #[unsafe(method(quitPane:))]
        fn quit_pane(&self, _item: &NSMenuItem) {
            self.ivars().selections.send(TrayAction::Quit);
        }
    }
);

impl TrayTarget {
    /// Makes the target on the main thread, with its ivars set before
    /// `NSObject`'s `init` runs, as a defined class is made.
    fn new(ivars: TargetIvars, marker: MainThreadMarker) -> Retained<TrayTarget> {
        // SAFETY: `TrayTarget` is a class of Pane's own, made on the main
        // thread the marker proves; the ivars are set before the
        // superclass initializes, and `NSObject`'s `init` returns a fully
        // initialized object of this class.
        let this = marker.alloc::<TrayTarget>().set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }
}

impl MacTray {
    /// Makes the adapter. Call it on the main thread, the one whose run
    /// loop the status item and its menu live on; `Err` says why it
    /// could not be made.
    pub fn start(selections: SelectionSender) -> Result<MacTray, String> {
        let Some(marker) = MainThreadMarker::new() else {
            return Err("Pane's menu bar item must be made on the main thread".into());
        };
        let target = TrayTarget::new(TargetIvars { selections }, marker);
        let state = State {
            bar: NSStatusBar::systemStatusBar(),
            target,
            item: None,
        };
        Ok(MacTray {
            state: Mutex::new(state),
        })
    }
}

/// One menu item: the platform's own label, the target's action, no key
/// equivalent — the tray's menu claims no keyboard binding of its own.
/// Called on the main thread.
fn menu_item(
    marker: MainThreadMarker,
    title: &str,
    action: objc2::runtime::Sel,
    target: &TrayTarget,
) -> Retained<NSMenuItem> {
    let title = NSString::from_str(title);
    let key = NSString::from_str("");
    // SAFETY: the initializer is `NSMenuItem`'s own; the title and key are
    // valid for it, and the selector is one `TrayTarget` implements.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            marker.alloc::<NSMenuItem>(),
            &title,
            Some(action),
            &key,
        )
    };
    // SAFETY: the target outlives the item — the adapter keeps it for as
    // long as the menu can send — and `AnyObject` is the root class the
    // parameter asks for.
    unsafe { item.setTarget(Some(target.as_super().as_super())) };
    item
}

impl Tray for MacTray {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn set_visible(&self, visible: bool) -> Result<(), TrayError> {
        let Ok(mut state) = self.state.lock() else {
            return Err(TrayError::Refused(
                "the menu bar entry was busy on a click".into(),
            ));
        };
        match (&state.item, visible) {
            // Repeating the state in effect succeeds, as the trait says;
            // a made item is shown and hidden in place, so showing it
            // again needs no new one.
            (Some(item), wanted) => {
                item.setVisible(wanted);
                Ok(())
            }
            (None, false) => Ok(()),
            (None, true) => {
                let Some(marker) = MainThreadMarker::new() else {
                    return Err(TrayError::Refused(
                        "Pane's menu bar item can only be changed on the main thread".into(),
                    ));
                };
                // The item's menu: Open Pane, Settings, Quit Pane, the
                // platform's own labels, each sending to the target.
                let menu = NSMenu::new(marker);
                let title = NSString::from_str("Pane");
                menu.setTitle(&title);
                for (label, action) in [
                    ("Open Pane", sel!(openPane:)),
                    ("Settings", sel!(showSettings:)),
                    ("Quit Pane", sel!(quitPane:)),
                ] {
                    menu.addItem(&menu_item(marker, label, action, &state.target));
                }
                // The item itself: as wide as its title asks, in the menu
                // bar's system status area, with that menu.
                let item = state.bar.statusItemWithLength(NSVariableStatusItemLength);
                item.setMenu(Some(&menu));
                if let Some(button) = item.button(marker) {
                    let title = NSString::from_str("Pane");
                    button.as_super().setTitle(&title);
                }
                item.setVisible(true);
                state.item = Some(item);
                Ok(())
            }
        }
    }
}

impl Drop for MacTray {
    /// Removes the item from the menu bar, if one was made; the bar and
    /// the target go with the adapter.
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock()
            && let Some(item) = state.item.take()
        {
            state.bar.removeStatusItem(&item);
        }
    }
}
