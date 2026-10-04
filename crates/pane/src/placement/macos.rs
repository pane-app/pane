//! The displays on macOS: the displays CoreGraphics lists, each with its
//! bounds in points (CoreGraphics' global display space, whose origin is
//! the primary display's top left), the main display, and the pointer's
//! position from a fresh event of the HID system. The display of the
//! active window — the one the user is working in, of whatever application
//! — is not told: no API of Pane's names another application's window
//! without a permission Pane does not ask for, so that choice is explained
//! rather than offered. Moving a window is `setFrameTopLeftPoint` on the
//! AppKit window behind the view, with the y flipped between the two
//! coordinate spaces (AppKit's origin is the primary display's bottom
//! left, CoreGraphics' its top left).

use core_graphics::display::{
    CGDirectDisplayID, CGDisplayBounds, CGGetActiveDisplayList, CGMainDisplayID,
};
use core_graphics::event::CGEvent;
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGRect;
use gpui::Window;
use objc2::encode::{Encode, Encoding};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::NSObject;
use pane_core::placement::{Display, DisplayId, DisplayLayout, Point, Rect, Size};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::Placement;

/// The macOS placement: the displays, the main one and the pointer, read
/// fresh each time the launcher opens.
pub(super) struct MacDisplays;

impl Placement for MacDisplays {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn layout(&self) -> DisplayLayout {
        // The displays this session has, as CoreGraphics lists them: at
        // most 32, as the API allows in one call.
        let mut ids: [CGDirectDisplayID; 32] = [0; 32];
        let mut count = 0;
        // SAFETY: `ids` is an array of the API's display ids, and `count`
        // is its count, as the API's contract requires.
        unsafe { CGGetActiveDisplayList(32, ids.as_mut_ptr(), &mut count) };
        let displays: Vec<Display> = ids[..count as usize]
            .iter()
            .map(|&id| {
                // SAFETY: `id` is one of the displays the call above
                // listed.
                let bounds = rect(unsafe { CGDisplayBounds(id) });
                Display {
                    id: DisplayId(id as u64),
                    // CoreGraphics names no work area: the whole display
                    // is the usable area, and the Dock and the menu bar
                    // are what the placement cannot account for.
                    usable: bounds,
                    bounds,
                }
            })
            .collect();
        DisplayLayout {
            displays,
            primary: Some(DisplayId(unsafe { CGMainDisplayID() } as u64)),
            pointer: pointer(),
            // Not told on macOS: no API of Pane's names another
            // application's window without a permission Pane does not ask
            // for. The page explains the choice instead of offering it.
            active: None,
        }
    }

    fn place(&self, window: &mut Window, bounds: Rect) -> Result<(), String> {
        // `Window` also has an inherent `window_handle` (GPUI's own
        // identifier), so the raw-window-handle trait is named rather than
        // called through the receiver.
        let handle = match HasWindowHandle::window_handle(window) {
            Ok(handle) => handle,
            Err(_) => {
                // No platform window exists to move — GPUI's test
                // platform; the placement's decision is what its tests
                // observe.
                return Ok(());
            }
        };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Ok(());
        };
        // SAFETY: `view` is the `NSView` GPUI built this window from, and
        // `window` is its `NSWindow`, held only for this call: the move
        // sets the window's top-left corner and nothing else.
        let view = handle.ns_view.cast::<NSObject>();
        let ns_window: Option<Retained<NSObject>> = unsafe { msg_send![view, window] };
        let Some(ns_window) = ns_window else {
            return Err("the view has no window to move".into());
        };
        // The AppKit space's origin is the primary display's bottom left;
        // the layout's (CoreGraphics') its top left — the y the window's
        // top left moves to is flipped against the primary's height.
        // SAFETY: the point is a `CGPoint` in AppKit's global space.
        let primary = unsafe { CGDisplayBounds(CGMainDisplayID()) };
        let point = TopLeftPoint {
            x: bounds.origin.x as f64,
            y: primary.size.height - bounds.origin.y as f64,
        };
        let _: () = unsafe { msg_send![&*ns_window, setFrameTopLeftPoint: point] };
        Ok(())
    }
}

/// The pointer's position in the layout's units (points, in the same
/// global display space the displays' bounds are in), as a fresh event of
/// the HID system reports it; `None` when none can be made.
fn pointer() -> Option<Point> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()?;
    let event = CGEvent::new(source).ok()?;
    let location = event.location();
    Some(Point {
        x: location.x as f32,
        y: location.y as f32,
    })
}

/// A CoreGraphics rectangle, in points, as the layout's own rectangle.
fn rect(place: CGRect) -> Rect {
    Rect {
        origin: Point {
            x: place.origin.x as f32,
            y: place.origin.y as f32,
        },
        size: Size {
            width: place.size.width as f32,
            height: place.size.height as f32,
        },
    }
}

/// A point in AppKit's global space, as `setFrameTopLeftPoint:` takes it:
/// the encoding of AppKit's own `NSPoint`, which is `CGPoint`.
#[repr(C)]
#[derive(Clone, Copy)]
struct TopLeftPoint {
    x: f64,
    y: f64,
}

// SAFETY: `TopLeftPoint` is a plain `#[repr(C)]` pair of doubles, the
// layout of AppKit's `NSPoint` and of CoreGraphics' `CGPoint`, whose
// encoding this names.
unsafe impl Encode for TopLeftPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}
