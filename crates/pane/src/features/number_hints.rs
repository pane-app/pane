//! The number hints (see the glossary's **Number hints**): holding Ctrl
//! alone for a moment shows the numbers Ctrl+1 to Ctrl+9 pick the
//! launcher's items with — the pinned home's first slots, then the first
//! rows — and the chord picks one.
//!
//! The hints are a look at the chords, and it ends the moment the user
//! does anything else: any other modifier, any key pressed, a scroll, a
//! key release, the window losing focus or being hidden
//! ([`LauncherWindow::end_numbers`]).
//!
//! What each digit picks is [`numbered`]'s, from the launcher's view and
//! the home's numbered slots; the `impl LauncherWindow` block below holds
//! the hold, the hints' slide and the pick the launcher window wires to
//! its key and modifier events.

use std::time::{Duration, Instant};

use gpui::{Context, ModifiersChangedEvent, Window};
use pane_core::{LauncherView, Screen};

use crate::app::LauncherWindow;
use crate::features::quick_slots;
use crate::ui::motion;

/// How long Ctrl must be held alone before the launcher's items show the
/// numbers Ctrl+1 to Ctrl+9 pick them with.
const NUMBERS_HOLD: Duration = Duration::from_millis(400);

/// The number hints Ctrl reveals: whether they show, the hold that will
/// show them, and their slide in or out.
#[derive(Default)]
pub(crate) struct Numbers {
    /// Whether the hints show: Ctrl has been held alone long enough.
    shown: bool,
    /// Whether Ctrl is held alone and the hold has not yet shown them.
    pending: bool,
    /// Counts holds, so a hold that ended does not show them later.
    generation: u64,
    /// Their slide in flight, if any, and what the last frame drew.
    pub(crate) reveal: Option<motion::Tween>,
    drawn: bool,
}

impl Numbers {
    /// Advances the hints' slide to the frame about to be drawn, at `now`
    /// (`reduced`: [`gpui::App::reduce_motion`]): their look, 0 hidden and
    /// 1 shown. The launcher's frame motion calls this each frame
    /// ([`crate::app::FrameMotion::frame`]).
    pub(crate) fn advance(&mut self, reduced: bool, now: Instant) -> f32 {
        let shown = self.shown;
        let changed = self.drawn != shown;
        self.drawn = shown;
        motion::advance_reveal(&mut self.reveal, shown, changed, reduced, now).unwrap_or(if shown {
            1.
        } else {
            0.
        })
    }
}

/// What Ctrl and a digit pick: a quick slot, or a row of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Numbered {
    Slot(usize),
    Row(usize),
}

/// What Ctrl and `digit` (1 to 9) pick on the screen `view` shows: while
/// the pinned home shows, the first numbers are `slots` (the home's
/// numbered slots, in order; see `LauncherWindow::numbered_slots`) and
/// the next ones the first rows; otherwise 1 to 9 are the first rows. Only
/// root search and a command's lists number their items.
pub(crate) fn numbered(view: &LauncherView, slots: &[usize], digit: usize) -> Option<Numbered> {
    if !matches!(
        view.screen,
        Screen::Root { .. } | Screen::Command | Screen::CommandSearch { .. }
    ) || !(1..=9).contains(&digit)
    {
        return None;
    }
    let home = quick_slots::home_shown(view);
    let slots = if home { slots } else { &[] };
    let picked = match slots.get(digit - 1) {
        Some(&slot) => Numbered::Slot(slot),
        None => Numbered::Row(digit - slots.len() - 1),
    };
    match picked {
        Numbered::Row(row) if row >= view.rows.len() => None,
        picked => Some(picked),
    }
}

/// The number row `index` of `view`'s list is picked with, if it has one
/// (see [`numbered`]).
pub(crate) fn row_number(view: &LauncherView, slots: &[usize], index: usize) -> Option<usize> {
    (1..=9).find(|&digit| numbered(view, slots, digit) == Some(Numbered::Row(index)))
}

impl LauncherWindow {
    /// Ctrl pressed or released: held alone, it shows the number hints
    /// once held for [`NUMBERS_HOLD`]; any other modifiers, or none, hide
    /// them at once.
    pub(crate) fn modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.modifiers;
        let alone = modifiers.control
            && !modifiers.alt
            && !modifiers.shift
            && !modifiers.platform
            && !modifiers.function;
        if !alone {
            self.end_numbers(cx);
            return;
        }
        if self.motion.numbers.pending || self.motion.numbers.shown {
            return;
        }
        self.motion.numbers.generation += 1;
        self.motion.numbers.pending = true;
        let generation = self.motion.numbers.generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NUMBERS_HOLD).await;
            this.update(cx, |this, cx| {
                if this.motion.numbers.pending && this.motion.numbers.generation == generation {
                    this.motion.numbers.pending = false;
                    this.motion.numbers.shown = true;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Ends a hold of Ctrl: the hints slide away, and a hold not yet long
    /// enough shows nothing.
    pub(crate) fn end_numbers(&mut self, cx: &mut Context<Self>) {
        self.motion.numbers.generation += 1;
        self.motion.numbers.pending = false;
        if std::mem::take(&mut self.motion.numbers.shown) {
            cx.notify();
        }
    }

    /// A key pressed in the launcher, before any chord sees it: the look
    /// at the numbers is over — the hints slide away if they had shown,
    /// and a hold not yet long enough shows nothing — because the user is
    /// pressing a chord (or typing), not reading the numbers. The chord's
    /// own digits pick through [`Self::pick_number`], which ends them too.
    pub(crate) fn chord_pressed(&mut self, cx: &mut Context<Self>) {
        self.end_numbers(cx);
    }

    /// What Ctrl and `digit` pick as the launcher is drawn: what
    /// [`numbered`] names, except a row while the window is collapsed,
    /// where the rows are hidden and only the pins are picked.
    pub(crate) fn number_target(&self, digit: usize) -> Option<Numbered> {
        let picked = numbered(&self.launcher.view(), &self.numbered_slots(), digit);
        match picked {
            Some(Numbered::Row(_)) if self.is_collapsed() => None,
            picked => picked,
        }
    }

    /// Picks what Ctrl and `digit` name (see [`Self::number_target`]): a
    /// quick slot is pressed as its chord always pressed it, a row is
    /// selected and activated. Whether something was picked.
    pub(crate) fn pick_number(
        &mut self,
        digit: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(picked) = self.number_target(digit) else {
            return false;
        };
        self.end_numbers(cx);
        match picked {
            Numbered::Slot(index) => self.press_quick_slot(index, window, cx),
            Numbered::Row(index) => {
                if self.actions.is_some() || self.menu.is_some() {
                    return true;
                }
                self.launcher.select(index);
                self.announcer.user_moved();
                self.activate_selected(window, cx);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use pane_core::{Row, Status};

    use super::*;

    /// A view of `screen` listing `count` rows.
    fn view(screen: Screen, count: usize) -> LauncherView {
        LauncherView {
            screen,
            title: String::new(),
            rows: (0..count)
                .map(|index| Row {
                    id: format!("row-{index}"),
                    title: format!("Row {index}"),
                    subtitle: None,
                    unavailable: None,
                })
                .collect(),
            selected: None,
            status: Status::Idle,
        }
    }

    /// The numbered pins of five or more: the first five, in their places
    /// (see `LauncherWindow::numbered_slots`).
    const STRIP: [usize; 5] = [0, 1, 2, 3, 4];

    #[test]
    fn the_pinned_home_takes_one_to_five_and_the_rows_follow() {
        // Five pins, or eight: only the first five are numbered, and the
        // rows take the digits after them.
        let home = view(
            Screen::Root {
                query: String::new(),
            },
            3,
        );
        assert_eq!(numbered(&home, &STRIP, 1), Some(Numbered::Slot(0)));
        assert_eq!(numbered(&home, &STRIP, 5), Some(Numbered::Slot(4)));
        assert_eq!(numbered(&home, &STRIP, 6), Some(Numbered::Row(0)));
        assert_eq!(numbered(&home, &STRIP, 8), Some(Numbered::Row(2)));
        // Past the rows, and outside 1 to 9, nothing is picked.
        assert_eq!(numbered(&home, &STRIP, 9), None);
        assert_eq!(numbered(&home, &STRIP, 0), None);
        assert_eq!(row_number(&home, &STRIP, 0), Some(6));
        assert_eq!(row_number(&home, &STRIP, 2), Some(8));
    }

    #[test]
    fn fewer_pins_than_five_leave_their_numbers_to_the_rows() {
        // Two pins: Ctrl+1 and Ctrl+2 are theirs, and the rows follow from
        // 3, up to 9.
        let home = view(
            Screen::Root {
                query: String::new(),
            },
            12,
        );
        let pinned = [0, 1];
        assert_eq!(numbered(&home, &pinned, 1), Some(Numbered::Slot(0)));
        assert_eq!(numbered(&home, &pinned, 2), Some(Numbered::Slot(1)));
        assert_eq!(numbered(&home, &pinned, 3), Some(Numbered::Row(0)));
        assert_eq!(numbered(&home, &pinned, 9), Some(Numbered::Row(6)));
        assert_eq!(row_number(&home, &pinned, 0), Some(3));
        assert_eq!(row_number(&home, &pinned, 6), Some(9));
        assert_eq!(row_number(&home, &pinned, 7), None);
        // Nothing pinned: the rows start at 1.
        assert_eq!(row_number(&home, &[], 0), Some(1));
    }

    #[test]
    fn past_the_fifth_pin_the_numbers_go_to_the_rows() {
        // Eight pins, of which the first five are numbered: Ctrl+6 to
        // Ctrl+9 are the first four rows, and pins 6 to 8 have none.
        let home = view(
            Screen::Root {
                query: String::new(),
            },
            12,
        );
        assert_eq!(numbered(&home, &STRIP, 6), Some(Numbered::Row(0)));
        assert_eq!(numbered(&home, &STRIP, 9), Some(Numbered::Row(3)));
        assert_eq!(row_number(&home, &STRIP, 3), Some(9));
        assert_eq!(row_number(&home, &STRIP, 4), None);
        assert!(
            (1..=9).all(|digit| numbered(&home, &STRIP, digit) != Some(Numbered::Slot(5))),
            "the sixth pin has no number"
        );
    }

    #[test]
    fn a_query_or_a_command_numbers_its_rows_from_one() {
        let search = view(
            Screen::Root {
                query: "notes".into(),
            },
            12,
        );
        assert_eq!(numbered(&search, &STRIP, 1), Some(Numbered::Row(0)));
        assert_eq!(numbered(&search, &STRIP, 9), Some(Numbered::Row(8)));
        assert_eq!(row_number(&search, &STRIP, 8), Some(9));
        assert_eq!(row_number(&search, &STRIP, 9), None);
        let command = view(Screen::Command, 2);
        assert_eq!(numbered(&command, &STRIP, 2), Some(Numbered::Row(1)));
        assert_eq!(numbered(&command, &STRIP, 3), None);
    }
}
