//! The keyboard's scrolling of a screen's long detail (#499): text that
//! scrolled only with the wheel — the details of a confirmation, a
//! preview or the error overlay, the detail pane beside a designed List,
//! Clipboard History's and Search Files' previews, a designed Detail, the
//! toast's full text — scrolls from the keyboard too.
//!
//! Raycast documents no key for it, so Pane takes the platform's text
//! convention: Shift+Up and Shift+Down scroll by a line's step, Shift+Page
//! Up and Shift+Page Down by the region's height, and the selection stays
//! where it is — plain Up and Down still move it. On a screen that is its
//! detail alone (a designed view with no List, a form Pane asks such as
//! What's New) there is no selection to move, so the plain keys scroll it
//! too, unless a control that moves with the arrows (a field, a slider,
//! a segmented control, a canvas) holds the keyboard. A key with nothing to
//! scroll goes on to the bindings: a search field's Shift+Up still
//! selects its text when the detail beside it fits.
//!
//! One region at a time is the screen's detail: the one the frame drew,
//! named by what it shows, so another record's preview starts at its top.
//! The keys are scrolled ahead of the key bindings (the window's
//! keystroke interceptor), since the search field beside a detail binds
//! Shift+Up and Shift+Down itself.

use std::cell::{Cell, RefCell};

use gpui::prelude::*;
use gpui::{Context, Div, Keystroke, ScrollHandle, Window, div, px};

use crate::app::LauncherWindow;
use crate::ui::theme::Theme;

/// What a Shift+Up or Shift+Down scrolls by, in px: about two lines of
/// the detail's text.
const LINE: f32 = 40.;

/// Which keys scroll the drawn region.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Keys {
    /// Shift with Up, Down, Page Up and Page Down: the region sits beside
    /// a selection the plain keys move.
    Shifted,
    /// The plain keys too: the region is the screen.
    Plain,
}

/// The screen's detail region: its scroll position, kept while the same
/// thing is shown, and whether this frame drew one.
#[derive(Default)]
pub(crate) struct DetailScroll {
    /// What the region shows, as its drawing named it.
    shown: RefCell<Option<String>>,
    /// Its scroll position.
    handle: RefCell<ScrollHandle>,
    /// The keys the region this frame drew takes; `None` while no region
    /// is drawn.
    drawn: Cell<Option<Keys>>,
}

impl DetailScroll {
    /// A new frame: no region is drawn until one asks for its handle.
    pub(crate) fn start_frame(&self) {
        self.drawn.set(None);
    }

    /// The handle the region showing `shown` tracks, scrolled by `keys`:
    /// the same one while it shows the same thing, a new one at the top
    /// when it shows something else.
    pub(crate) fn region(&self, shown: &str, keys: Keys) -> ScrollHandle {
        let mut current = self.shown.borrow_mut();
        if current.as_deref() != Some(shown) {
            *current = Some(shown.to_owned());
            *self.handle.borrow_mut() = ScrollHandle::new();
        }
        self.drawn.set(Some(keys));
        self.handle.borrow().clone()
    }

    /// The region the last frame drew, with the keys it takes.
    fn drawn(&self) -> Option<(ScrollHandle, Keys)> {
        let keys = self.drawn.get()?;
        Some((self.handle.borrow().clone(), keys))
    }
}

/// Scrolls `handle` for `key`: a step up or down, or a page. `false` when
/// it has nothing to scroll — its content fits.
fn step(handle: &ScrollHandle, key: &str) -> bool {
    let most = f32::from(handle.max_offset().y);
    if most <= 0. {
        return false;
    }
    let page = (f32::from(handle.bounds().size.height) - LINE).max(LINE);
    let by = match key {
        "up" => -LINE,
        "down" => LINE,
        "pageup" => -page,
        "pagedown" => page,
        _ => return false,
    };
    // The offset is the content's, negative as it scrolls down.
    let mut offset = handle.offset();
    let scrolled = (-f32::from(offset.y) + by).clamp(0., most);
    offset.y = px(-scrolled);
    handle.set_offset(offset);
    true
}

/// How far `handle` is scrolled, in px from its top.
fn scrolled(handle: &ScrollHandle) -> f32 {
    -f32::from(handle.offset().y)
}

impl LauncherWindow {
    /// Scrolls the screen's detail for `keystroke`, when it is one of the
    /// detail's keys and there is something to scroll: whether it did,
    /// so the window's keystroke interceptor stops the key there.
    pub(crate) fn scroll_detail_by_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = keystroke.key.as_str();
        if !matches!(key, "up" | "down" | "pageup" | "pagedown") {
            return false;
        }
        let modifiers = keystroke.modifiers;
        let shifted = if modifiers == gpui::Modifiers::shift() {
            true
        } else if modifiers == gpui::Modifiers::default() {
            false
        } else {
            return false;
        };
        let Some(region) = self.scrolled_region(shifted, window) else {
            return false;
        };
        let moved = step(&region, key);
        if moved {
            cx.notify();
        }
        moved
    }

    /// The region the detail's keys scroll now, `shifted` or not: the
    /// toast's open details first, which hold the keyboard; else the
    /// screen's detail as the last frame drew it — or a designed tree's
    /// own scroll region, a Detail's content scrolling inside the pane —
    /// whichever overflows, the tallest when several do. `None` while a
    /// popup or a confirmation holds the keys, or a field keeps them for
    /// its own text.
    fn scrolled_region(&self, shifted: bool, window: &Window) -> Option<ScrollHandle> {
        if let Some(details) = self.toast_details_scroll() {
            return shifted.then_some(details);
        }
        if self.actions.is_some() || self.menu.is_some() || self.launcher.confirmation().is_some() {
            return None;
        }
        let (handle, keys) = self.detail_scroll.drawn()?;
        // A designed view's field keeps Shift and the arrows for its own
        // text (a text area selects by line), and a control that moves
        // with the arrows keeps the plain ones.
        if shifted && self.designed_takes_backspace(window) {
            return None;
        }
        if !shifted && (keys != Keys::Plain || self.designed_takes_arrows(window)) {
            return None;
        }
        std::iter::once(handle)
            .chain(self.designed_scroll_regions())
            .filter(|region| f32::from(region.max_offset().y) > 0.)
            .max_by(|one, other| {
                f32::from(one.bounds().size.height)
                    .total_cmp(&f32::from(other.bounds().size.height))
            })
    }

    /// The footer's hint while a Markdown link holds the keyboard (#499):
    /// the address Enter opens, as a browser's status bar shows it.
    pub(crate) fn footer_link(&self, window: &Window, theme: &Theme) -> Option<Div> {
        let url = self.focused_link_url(window)?;
        let selector = format!("footer-link-{url}");
        Some(
            div()
                .debug_selector(move || selector)
                .min_w(px(0.))
                .truncate()
                .text_size(theme.typography.footer_size)
                .text_color(theme.text_tertiary)
                .child(url),
        )
    }

    /// Test support: how far the region the detail's keys scroll is
    /// scrolled, in px from its top — the one Shift+Down would scroll now.
    /// `None` when nothing the keys scroll overflows.
    #[doc(hidden)]
    pub fn detail_scrolled(&self, window: &Window) -> Option<f32> {
        self.scrolled_region(true, window)
            .map(|region| scrolled(&region))
    }
}
