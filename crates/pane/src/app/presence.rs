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

/// How long after an accepted Open Pane press another press of the same
/// binding is treated as the repeat of a key still held, not a new press.
/// The Windows and X11 adapters stop the system's key repeat at its source
/// (`MOD_NOREPEAT`, detectable auto-repeat); macOS's Carbon hot keys
/// report a held key again, so the window keeps the guard itself. A
/// genuine second press after this long toggles again.
const OPEN_PANE_REPEAT: Duration = Duration::from_millis(600);

/// A window size in logical pixels, as the window reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowSize {
    pub(crate) width: f32,
    pub(crate) height: f32,
}

/// What an Open Pane press does (see [`Presence::press`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Press {
    /// The repeat of a key still held: nothing.
    Repeat,
    /// Hide the launcher: it is shown and has the focus.
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
    /// The window's size now.
    pub(crate) current: WindowSize,
    /// The size expanding gives back when no expanded size was kept.
    pub(crate) default_expanded: WindowSize,
}

/// The launcher window's presence (see the module docs).
#[derive(Default)]
pub(crate) struct Presence {
    /// When the Open Pane hotkey was last accepted, so the repeats of a
    /// held key do not toggle again and again (see [`OPEN_PANE_REPEAT`]).
    open_pane_press: Option<Instant>,
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

    /// An Open Pane press at `now`, with the window `active` or not: a
    /// press within [`OPEN_PANE_REPEAT`] of the last one accepted is the
    /// repeat of a key still held; otherwise the launcher hides when it is
    /// shown and has the focus, and is summoned when it does not.
    pub(crate) fn press(&mut self, now: Instant, active: bool) -> Press {
        if self
            .open_pane_press
            .is_some_and(|last| now.duration_since(last) < OPEN_PANE_REPEAT)
        {
            return Press::Repeat;
        }
        self.open_pane_press = Some(now);
        if !self.hidden && active {
            Press::Hide
        } else {
            Press::Summon
        }
    }

    /// The launcher was hidden at `now`.
    pub(crate) fn hide(&mut self, now: Instant) {
        self.hidden = true;
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
        let with_pins = fit.bar + fit.pin_row;
        // The collapsed height this frame calls for.
        let target = if fit.collapses && fit.shows_pins {
            with_pins
        } else {
            fit.bar
        };
        let size = fit.current;
        let near = |height: f32| (size.height - height).abs() < 1.;
        // Either collapsed height: a size never to keep as the expanded one.
        let is_bar = near(fit.bar) || near(with_pins);
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
    fn a_press_within_600ms_of_the_last_accepted_one_is_a_repeat() {
        let start = Instant::now();
        let mut presence = Presence::default();
        assert_eq!(presence.press(start, true), Press::Hide);
        presence.hide(start);
        let held = start + Duration::from_millis(599);
        assert_eq!(presence.press(held, true), Press::Repeat);
        // A repeat is not accepted, so the guard still counts from the
        // first press.
        let again = start + Duration::from_millis(600);
        assert_eq!(presence.press(again, true), Press::Summon);
    }

    #[test]
    fn a_press_hides_only_a_launcher_shown_with_the_focus() {
        let mut now = Instant::now();
        let mut presence = Presence::default();
        let mut press = |presence: &mut Presence, active| {
            now += OPEN_PANE_REPEAT;
            presence.press(now, active)
        };
        assert_eq!(
            press(&mut presence, false),
            Press::Summon,
            "shown, no focus"
        );
        assert_eq!(press(&mut presence, true), Press::Hide, "shown, focused");
        presence.hide(Instant::now());
        assert_eq!(press(&mut presence, true), Press::Summon, "hidden");
        assert_eq!(press(&mut presence, false), Press::Summon, "hidden");
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
