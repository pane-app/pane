//! The launcher window's presence: whether it is shown, when it was
//! hidden, how the Open Pane hotkey's presses toggle it, and the size the
//! compact window mode collapses it to and gives back.
//!
//! [`Presence`] holds the state and makes the decisions, from plain
//! inputs — the time, whether the window is active, the reopening choice,
//! the window's size — and answers with what to do: hide, summon (popping
//! to root search or restoring the view), resize to a size, place at a
//! size. It has no GPUI in it, so its rules are tested on their own; the
//! launcher window ([`super::LauncherWindow`]) keeps the GPUI calls that
//! carry the answers out (`set_visible`, `activate_window`, `resize`, the
//! placement's `place`).

use std::time::{Duration, Instant};

/// A window size in logical pixels, as the window reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowSize {
    pub(crate) width: f32,
    pub(crate) height: f32,
}

/// What an Open Pane press does (see [`Presence::press`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Press {
    /// Hide the launcher: it is shown and has the focus, or was summoned
    /// and has not been seen to lose it since.
    Hide,
    /// Show and focus the launcher: it is hidden, or shown without the
    /// focus (another application's, or the Settings window's — its focus
    /// does not count).
    Summon,
}

/// What fitting the window to the window mode reads, for one frame (see
/// [`Presence::fit`]).
pub(crate) struct Fit {
    /// Whether the launcher shows only its search field this frame.
    pub(crate) collapses: bool,
    /// Whether the collapsed window shows the pins' row under its search
    /// field; asked only while it collapses.
    pub(crate) shows_pins: bool,
    /// The search field's height: the collapsed window's.
    pub(crate) bar: f32,
    /// The pins' row's height, added under the bar while it shows.
    pub(crate) pin_row: f32,
    /// Whether the collapsed window shows Restart to Update Pane under its
    /// search field (ADR 0050); asked only while it collapses.
    pub(crate) shows_update: bool,
    /// That row's height, added under the bar while it shows.
    pub(crate) update_row: f32,
    /// The window's size now.
    pub(crate) current: WindowSize,
    /// The size expanding gives back when no expanded size was kept.
    pub(crate) default_expanded: WindowSize,
}

/// The launcher window's presence (see the module docs).
#[derive(Default)]
pub(crate) struct Presence {
    /// The summon still bringing the launcher forward, by its number: the
    /// system brings a window forward on its own time, so a press that
    /// comes before the window is active — a quick second press, a press
    /// while it opens — hides the launcher the first one summoned rather
    /// than summoning it again (#467).
    summoning: Option<u64>,
    /// The number the last summon was given.
    summons: u64,
    /// Whether the launcher window is hidden by the Open Pane hotkey —
    /// hidden, not closed: Pane keeps running, and the next press shows
    /// the same window and the same launcher again. Drives the toggle's
    /// decision together with the window's focus, so what the hotkey does
    /// is the same whatever the platform reports about a hidden window.
    hidden: bool,
    /// When the launcher was last hidden, for the Launcher page's pop to
    /// root search choice.
    hidden_at: Option<Instant>,
    /// Whether the last frame drew the launcher collapsed to its search
    /// field (the compact window mode), and the size it had before.
    collapsed: Option<bool>,
    expanded_size: Option<WindowSize>,
}

impl Presence {
    /// Whether the launcher window is hidden.
    pub(crate) fn hidden(&self) -> bool {
        self.hidden
    }

    /// An Open Pane press, with the window `active` or not: the launcher
    /// hides when it is shown and has the focus — or was just summoned,
    /// its activation not seen yet — and is summoned when it does not.
    /// Every press is a toggle, however soon after the last one: the
    /// system adapters report a held key once (`MOD_NOREPEAT` and the
    /// hook's repeat rule on Windows, the grab's held state on X11, the
    /// Carbon hot key's release on macOS), so a press here is always the
    /// user's (#467).
    pub(crate) fn press(&mut self, active: bool) -> Press {
        if !self.hidden && (active || self.summoning.is_some()) {
            Press::Hide
        } else {
            Press::Summon
        }
    }

    /// The launcher is being summoned: until the summon this answers is
    /// over ([`Presence::brought_forward`]), a press hides it.
    pub(crate) fn summoned(&mut self) -> u64 {
        self.summons += 1;
        self.summoning = Some(self.summons);
        self.summons
    }

    /// The summon `summon` has brought the launcher forward, or tried to:
    /// from now on the window's own focus decides what a press does. A
    /// later summon's is not ended by an earlier one's.
    pub(crate) fn brought_forward(&mut self, summon: u64) {
        if self.summoning == Some(summon) {
            self.summoning = None;
        }
    }

    /// The window became active: whatever summon brought it, it is over.
    pub(crate) fn activated(&mut self) {
        self.summoning = None;
    }

    /// The launcher was hidden at `now`.
    pub(crate) fn hide(&mut self, now: Instant) {
        self.hidden = true;
        self.summoning = None;
        self.hidden_at = Some(now);
    }

    /// The launcher is to be shown: whether it was hidden, so the window
    /// is to be made visible and placed (a shown one stays as it is).
    pub(crate) fn show(&mut self) -> bool {
        std::mem::replace(&mut self.hidden, false)
    }

    /// Whether a summon at `now` starts from root search, by the reopening
    /// choice's `pops_after` (`None`: never). It counts the time hidden: a
    /// launcher brought forward while still shown (another application
    /// had the focus) was not hidden at all, so only "immediately" pops it
    /// — a delay counts the time hidden, not the time since some earlier
    /// hide. Ask before showing it.
    pub(crate) fn pops_to_root(&self, pops_after: Option<Duration>, now: Instant) -> bool {
        let away = match self.hidden_at {
            Some(hidden) if self.hidden => now.saturating_duration_since(hidden),
            _ => Duration::ZERO,
        };
        pops_after.is_some_and(|after| away >= after)
    }

    /// Whether the last frame drew the launcher collapsed to its search
    /// field (and the pins' row, if shown): its rows are hidden then.
    pub(crate) fn is_collapsed(&self) -> bool {
        self.collapsed == Some(true)
    }

    /// The size the launcher is placed by, the window's `current` one
    /// otherwise: collapsed to its search field, the launcher is placed as
    /// its expanded size would be, so it grows downward from where it is.
    pub(crate) fn placement_size(&self, current: WindowSize) -> WindowSize {
        match (self.collapsed, self.expanded_size) {
            (Some(true), Some(expanded)) => expanded,
            _ => current,
        }
    }

    /// Fits the window to the window mode for one frame: the size to
    /// resize it to, if any. Collapsing to the search field's height — and
    /// the pins' row under it, while it shows — keeps the size the window
    /// had, which expanding gives back. The window's own height is what is
    /// compared, so a size another view gave it (Clipboard History's) is
    /// fitted too, and so is a collapsed height the switch or the pins no
    /// longer call for.
    pub(crate) fn fit(&mut self, fit: Fit) -> Option<WindowSize> {
        // The collapsed height this frame calls for: the bar, and under it
        // the update's row and the pins' row while each shows.
        let target = fit.bar
            + if fit.collapses && fit.shows_update {
                fit.update_row
            } else {
                0.
            }
            + if fit.collapses && fit.shows_pins {
                fit.pin_row
            } else {
                0.
            };
        let size = fit.current;
        let near = |height: f32| (size.height - height).abs() < 1.;
        // Any collapsed height: a size never to keep as the expanded one.
        let is_bar = [0., fit.update_row].into_iter().any(|update| {
            [0., fit.pin_row]
                .into_iter()
                .any(|pins| near(fit.bar + update + pins))
        });
        let resize = if fit.collapses && !near(target) {
            if !is_bar {
                self.expanded_size = Some(size);
            }
            Some(WindowSize {
                width: size.width,
                height: target,
            })
        } else if !fit.collapses && is_bar && self.collapsed == Some(true) {
            Some(self.expanded_size.unwrap_or(fit.default_expanded))
        } else {
            None
        };
        self.collapsed = Some(fit.collapses);
        resize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: f32 = 64.;
    const PIN_ROW: f32 = 48.;
    const UPDATE_ROW: f32 = 60.;
    const EXPANDED: WindowSize = WindowSize {
        width: 760.,
        height: 518.,
    };
    const DEFAULT: WindowSize = WindowSize {
        width: 700.,
        height: 500.,
    };

    fn fit(collapses: bool, shows_pins: bool, current: WindowSize) -> Fit {
        Fit {
            collapses,
            shows_pins,
            bar: BAR,
            pin_row: PIN_ROW,
            shows_update: false,
            update_row: UPDATE_ROW,
            current,
            default_expanded: DEFAULT,
        }
    }

    fn size(height: f32) -> WindowSize {
        WindowSize {
            width: EXPANDED.width,
            height,
        }
    }

    #[test]
    fn a_press_hides_only_a_launcher_shown_with_the_focus() {
        let mut presence = Presence::default();
        assert_eq!(presence.press(false), Press::Summon, "shown, no focus");
        assert_eq!(presence.press(true), Press::Hide, "shown, focused");
        presence.hide(Instant::now());
        assert_eq!(presence.press(true), Press::Summon, "hidden");
        assert_eq!(presence.press(false), Press::Summon, "hidden");
    }

    #[test]
    fn every_press_toggles_however_soon_it_follows_the_last() {
        let mut presence = Presence::default();
        presence.hide(Instant::now());
        // Pressed again and again with no time between: each press is
        // the user's, and toggles. The summoned launcher's activation is
        // not seen yet when the next press comes, and it still hides.
        for _ in 0..3 {
            assert_eq!(presence.press(false), Press::Summon);
            assert!(presence.show());
            presence.summoned();
            assert_eq!(presence.press(false), Press::Hide);
            presence.hide(Instant::now());
        }
        // A summon over before the next press: the window's focus
        // decides again.
        assert_eq!(presence.press(false), Press::Summon);
        presence.show();
        let summon = presence.summoned();
        presence.brought_forward(summon);
        assert_eq!(presence.press(true), Press::Hide);
    }

    #[test]
    fn a_summoned_launcher_that_lost_the_focus_is_brought_forward_again() {
        let mut presence = Presence::default();
        presence.hide(Instant::now());
        assert_eq!(presence.press(false), Press::Summon);
        presence.show();
        presence.summoned();
        // An earlier summon's end does not end a later one's.
        let first = presence.summoned();
        let second = presence.summoned();
        presence.brought_forward(first);
        assert_eq!(presence.press(false), Press::Hide, "the second still runs");
        // It came forward, then another window took the focus: a press
        // brings it forward rather than hiding it.
        presence.brought_forward(second);
        assert_eq!(presence.press(false), Press::Summon);
        presence.summoned();
        presence.activated();
        assert_eq!(presence.press(false), Press::Summon, "active, then not");
    }

    #[test]
    fn showing_says_whether_the_launcher_was_hidden() {
        let mut presence = Presence::default();
        assert!(!presence.show(), "shown already");
        presence.hide(Instant::now());
        assert!(presence.hidden());
        assert!(presence.show(), "it was hidden");
        assert!(!presence.hidden());
        assert!(!presence.show());
    }

    #[test]
    fn a_summon_pops_to_root_once_the_launcher_was_hidden_for_the_delay() {
        let hidden = Instant::now();
        let mut presence = Presence::default();
        presence.hide(hidden);
        let after = |secs| hidden + Duration::from_secs(secs);
        // Immediately: at once.
        assert!(presence.pops_to_root(Some(Duration::ZERO), hidden));
        // Never.
        assert!(!presence.pops_to_root(None, after(3600)));
        // After 90 seconds, and after 3 minutes.
        for delay in [90, 180] {
            let pops_after = Some(Duration::from_secs(delay));
            assert!(!presence.pops_to_root(pops_after, after(delay - 1)));
            assert!(presence.pops_to_root(pops_after, after(delay)));
        }
    }

    #[test]
    fn a_launcher_never_hidden_since_it_was_shown_pops_only_immediately() {
        let hidden = Instant::now();
        let mut presence = Presence::default();
        let later = hidden + Duration::from_secs(600);
        // Never hidden at all.
        assert!(presence.pops_to_root(Some(Duration::ZERO), later));
        assert!(!presence.pops_to_root(Some(Duration::from_secs(90)), later));
        // Hidden once, shown since: the old hide does not count.
        presence.hide(hidden);
        presence.show();
        assert!(presence.pops_to_root(Some(Duration::ZERO), later));
        assert!(!presence.pops_to_root(Some(Duration::from_secs(90)), later));
    }

    #[test]
    fn collapsing_takes_the_bar_or_the_bar_and_the_pins_row() {
        let mut presence = Presence::default();
        assert_eq!(presence.fit(fit(true, false, EXPANDED)), Some(size(BAR)));
        assert!(presence.is_collapsed());
        // The pins' row is turned on: the collapsed window grows by it.
        assert_eq!(
            presence.fit(fit(true, true, size(BAR))),
            Some(size(BAR + PIN_ROW))
        );
        // Already at that height: nothing to do.
        assert_eq!(presence.fit(fit(true, true, size(BAR + PIN_ROW))), None);
        // And off again: back to the bar.
        assert_eq!(
            presence.fit(fit(true, false, size(BAR + PIN_ROW))),
            Some(size(BAR))
        );
    }

    #[test]
    fn a_ready_update_adds_its_row_under_the_bar() {
        let mut presence = Presence::default();
        let with_update = |shows_pins, current| Fit {
            shows_update: true,
            ..fit(true, shows_pins, current)
        };
        assert_eq!(
            presence.fit(with_update(false, EXPANDED)),
            Some(size(BAR + UPDATE_ROW))
        );
        // Above the pins' row, when it shows too.
        assert_eq!(
            presence.fit(with_update(true, size(BAR + UPDATE_ROW))),
            Some(size(BAR + UPDATE_ROW + PIN_ROW))
        );
        // The update gone (Pane restarted into it, say): the bar and the
        // pins again, and no collapsed height is kept as the expanded one.
        assert_eq!(
            presence.fit(fit(true, true, size(BAR + UPDATE_ROW + PIN_ROW))),
            Some(size(BAR + PIN_ROW))
        );
        assert_eq!(
            presence.fit(fit(false, false, size(BAR + PIN_ROW))),
            Some(EXPANDED)
        );
    }

    #[test]
    fn expanding_gives_back_the_size_kept_when_collapsing() {
        let mut presence = Presence::default();
        assert_eq!(presence.fit(fit(false, false, EXPANDED)), None);
        assert!(!presence.is_collapsed());
        presence.fit(fit(true, false, EXPANDED));
        // A collapsed height is never kept as the expanded one.
        presence.fit(fit(true, true, size(BAR)));
        assert_eq!(
            presence.fit(fit(false, false, size(BAR + PIN_ROW))),
            Some(EXPANDED)
        );
        assert!(!presence.is_collapsed());
        // Expanded already: nothing to give back.
        assert_eq!(presence.fit(fit(false, false, EXPANDED)), None);
    }

    #[test]
    fn expanding_with_no_kept_size_takes_the_default() {
        let mut presence = Presence::default();
        // Opened at the bar's height, collapsed: there was no expanded size.
        presence.fit(fit(true, false, size(BAR)));
        assert_eq!(presence.fit(fit(false, false, size(BAR))), Some(DEFAULT));
    }

    #[test]
    fn a_collapsed_launcher_is_placed_at_its_expanded_size() {
        let mut presence = Presence::default();
        assert_eq!(presence.placement_size(EXPANDED), EXPANDED);
        presence.fit(fit(true, false, EXPANDED));
        assert_eq!(presence.placement_size(size(BAR)), EXPANDED);
        presence.fit(fit(false, false, size(BAR)));
        assert_eq!(presence.placement_size(EXPANDED), EXPANDED);
    }
}
