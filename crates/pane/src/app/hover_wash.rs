//! The window's hover washes, and the fade each leaves behind (#245,
//! ADR 0035): where hovering does not move the selection — a command's
//! list rows, root search's rows under an open overlay, the split view's
//! rows, the extension log's, pinned slots, the compact pins, the
//! footer-family buttons, the Pane menu's entries — the surface under the
//! pointer takes the theme's hover wash at once, and when the pointer
//! leaves, the wash fades out over [`motion::HOVER_FADE`]. Where hovering
//! *does* move the selection (root search's free rows, the Actions
//! panel's entries), no hover wash is drawn at all: the selection wash
//! shows, and it never fades.
//!
//! Every surface that takes the fade names itself a [`Spot`] — the same
//! name its render knows it by — reports the pointer's arrivals and
//! departures through [`HoverWashes::set`] from its `.on_hover` listener,
//! and reads its wash's strength through [`HoverWashes::look`] as it
//! draws. The strength is 1 under the pointer (the wash arrived at once),
//! the running fade's value while one is in flight, and 0 otherwise;
//! arriving again cancels the fade, so a quick return shows the full
//! wash, not a fade-in. [`HoverWashes::advance`] runs once per frame, from
//! [`crate::app::frame_motion::FrameMotion::frame`], which is also what
//! keeps the frames coming while a fade runs and requests none once the
//! last has settled — the shared frame discipline
//! (see [`crate::ui::motion`]). Under reduced motion no fade is kept: the
//! wash leaves at once.

use std::time::Instant;

use gpui::Context;

use crate::app::LauncherWindow;
use crate::ui::motion::{self, Tween};

/// One surface whose hover wash the window tracks, named the way its
/// render names it. `Copy`, so a row's hover listener and its render
/// share one value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Spot {
    /// A row of the launcher's result list, whatever the screen, at its
    /// index in the list's rows.
    Row(usize),
    /// A pinned slot of the home — a tile of the strip or a row of the
    /// vertical layout, whichever the Launcher page chose — at its index.
    Slot(usize),
    /// A pin of the compact window, at its index.
    Pin(usize),
    /// An entry of the footer's Pane menu, at its index.
    MenuItem(usize),
    /// A row of the split view's list (the clipboard history's records,
    /// a file search's files), at its index.
    Clip(usize),
    /// A line of the extension log, at its index.
    Log(usize),
    /// One of the footer-family buttons, by the element id it is drawn
    /// with: the mark, the primary action, a toast's or a confirmation's
    /// button, the split view's, the log's.
    Button(&'static str),
}

/// The hover washes in flight, as the module docs describe. One surface
/// is hovered at a time; the washes the pointer leaves behind each keep
/// their own fade, so a sweep across a list leaves every row's wash
/// fading on its own timeline (each no longer than
/// [`motion::HOVER_FADE`]).
#[derive(Default)]
pub(crate) struct HoverWashes {
    /// What the pointer is over now, if anything.
    hovered: Option<Spot>,
    /// The washes the pointer has left behind, each fading out: which
    /// surface, and the tween of its strength.
    exits: Vec<(Spot, Tween)>,
}

impl HoverWashes {
    /// The surface `spot` reports the pointer arrived (`over`) or left.
    /// An arrival shows the wash at once, cancelling any fade of the same
    /// surface (the wash never fades in); since the pointer moved there
    /// from another surface (or none), the one it left starts its fade
    /// now — its own departure event, which the platform may deliver
    /// after this arrival, then finds the fade in flight and starts
    /// nothing. A departure starts a fade for the surface the pointer
    /// was over; one for any other surface is stale — its fade is
    /// already running, or its wash was never showing.
    pub(crate) fn set(&mut self, spot: Spot, over: bool, cx: &mut Context<LauncherWindow>) {
        let now = cx.background_executor().now();
        if over {
            if self.hovered == Some(spot) {
                return;
            }
            if let Some(left) = self.hovered.replace(spot) {
                self.exits.retain(|(other, _)| *other != left);
                self.exits.push((left, motion::hover_exit(now)));
            }
            self.exits.retain(|(other, _)| *other != spot);
            cx.notify();
        } else if self.hovered == Some(spot) {
            self.hovered = None;
            self.exits.retain(|(other, _)| *other != spot);
            self.exits.push((spot, motion::hover_exit(now)));
            cx.notify();
        }
    }

    /// The strength of `spot`'s hover wash as it is drawn at `now`: full
    /// under the pointer, the running fade's value behind it, nothing
    /// otherwise.
    pub(crate) fn look(&self, spot: Spot, now: Instant) -> f32 {
        if self.hovered == Some(spot) {
            1.
        } else {
            self.exits
                .iter()
                .find(|(left, _)| *left == spot)
                .map_or(0., |(_, fade)| fade.value(now).clamp(0., 1.))
        }
    }

    /// Advances the fades to the frame about to be drawn, at `now`, and
    /// returns whether any is still in flight — the one thing that keeps
    /// the frames coming for them. Reduced motion settles them all at
    /// once, as every transition's reduced motion does.
    pub(crate) fn advance(&mut self, reduced: bool, now: Instant) -> bool {
        if reduced {
            self.exits.clear();
        } else {
            self.exits.retain(|(_, fade)| !fade.finished(now));
        }
        !self.exits.is_empty()
    }
}
