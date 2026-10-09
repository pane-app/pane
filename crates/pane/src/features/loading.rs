//! The loading bar (#248, ADR 0035): the one-pixel line along the rule
//! under the search field — root search's or an opened command's, the one
//! rule `crate::features::root_search::search_header` draws — while work
//! the user is waiting for has outlasted a moment. A soft highlight
//! sweeps across the line, one pass every
//! [`LOADING_SWEEP`](crate::ui::motion); the line fades in once the work
//! has run [`LOADING_AFTER`](crate::ui::motion) and out again once it
//! ends, so work a moment's wait outruns shows nothing at all — "a
//! loading bar that flashes for an instant answer is noise". It replaces
//! the footer's "Running…" text for such work; work Pane describes in
//! words in the footer (a development build, an acquisition, an update)
//! keeps its words. The busy state is announced through the footer's
//! status announcement only once the threshold has passed, so a quick
//! action is never announced as busy (see
//! [`crate::features::announcer`]).
//!
//! Whether the work is pending, and since when, is the core's to say:
//! [`crate::app::LauncherWindow::render`] reads
//! `pane_core::Launcher::pending_since` each frame and hands it to
//! [`Loading::advance`], which the window's frame motion calls (see
//! [`crate::app::frame_motion`]). The window owns the drawing and the
//! threshold — this module is the window's bookkeeping and the element,
//! as the number hints' module holds theirs; the policy spans themselves
//! live in `crate::ui::motion`, the one place the launcher's timings come
//! from.
//!
//! Under reduced motion the line is still, at partial strength
//! ([`LOADING_STILL`](crate::ui::motion)), and asks for no frame.
//!
//! Presentation only: behavior is attached by `crate::app::LauncherWindow`.

use std::time::{Duration, Instant};

use gpui::{
    ColorExt, Context, Div, Pixels, Task, div, linear_color_stop, linear_gradient, prelude::*, px,
    relative,
};

use crate::app::LauncherWindow;
use crate::ui::motion;
use crate::ui::theme::Theme;

/// The loading bar's state, kept by the window: which stretch of
/// waited-for work it is showing, its fade in flight and its sweep.
#[derive(Default)]
pub(crate) struct Loading {
    /// The core's stamp of the pending work this stretch: an invoked
    /// action's, command call's or command search's `Running { since }`,
    /// or root search's wait on its providers. A different stamp is a new
    /// stretch of work; `None` is the work's end.
    stamp: Option<Instant>,
    /// When this stretch began on the window's clock. The core stamps its
    /// own, which on native is the same clock; the lead between the core
    /// stamping the work and this window seeing it is taken off, so the
    /// count begins at the work, not at the frame that first drew it —
    /// and the test platform's controlled clock then holds the whole
    /// count, so tests move it.
    started: Option<Instant>,
    /// The bar's strength in flight, 0 hidden, 1 shown (see
    /// [`Loading::advance`]).
    fade: Option<motion::Tween>,
    /// When the sweep's pass began, on the window's clock: the first frame
    /// the line showed.
    swept: Option<Instant>,
    /// The strength the last frame settled at, so a change starts the
    /// fade.
    drawn: f32,
    /// The bar the last frame drew, for tests (see
    /// [`crate::app::LauncherWindow::loading_presentation`]). Test and
    /// debug builds only.
    #[cfg(any(test, debug_assertions))]
    pub(crate) drawn_bar: Option<LoadingBar>,
    /// When the window must draw again for the bar, if it waits: the
    /// threshold, while work runs beneath it. Kept beside its task, as
    /// the announcer keeps its wake.
    pub(crate) wake_at: Option<Instant>,
    _wake: Option<Task<()>>,
}

/// What one frame draws of the loading bar (see [`Loading::advance`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LoadingBar {
    /// The line's strength, 0 hidden to 1 shown.
    pub(crate) strength: f32,
    /// The sweep's highlight along the line: its left edge and its width,
    /// as shares of the line. `None` when the line is still (reduced
    /// motion): partial strength, no sweep.
    pub(crate) sweep: Option<(f32, f32)>,
}

/// What one frame drew of the loading bar, and what it waits for (see
/// [`Loading::advance`]).
pub(crate) struct LoadingFrame {
    /// The bar this frame draws, if anything: `None` draws no line at all
    /// — the resting rule is all the search field shows.
    pub(crate) bar: Option<LoadingBar>,
    /// Whether the window must draw again for the bar: a fade in flight,
    /// or the sweep running.
    pub(crate) animating: bool,
    /// Whether the waited-for work has outlasted the threshold — the busy
    /// state the footer's announcement may speak, and only then (#248).
    pub(crate) busy: bool,
    /// When the window must draw again for the bar, if it waits: the
    /// threshold, while work runs beneath it. Nothing else draws that
    /// frame, so it has to be asked for (see
    /// [`LauncherWindow::wake_loading`]).
    pub(crate) wake: Option<Instant>,
}

impl Loading {
    /// Advances the bar to the frame about to be drawn, at `now`, for the
    /// work `pending` (the core's
    /// [`pane_core::Launcher::pending_since`]: when the waited-for work
    /// began, or `None` when none is). `reduced` is
    /// [`gpui::App::reduce_motion`], under which the line shows at
    /// partial strength without sweeping.
    pub(crate) fn advance(
        &mut self,
        pending: Option<Instant>,
        reduced: bool,
        now: Instant,
    ) -> LoadingFrame {
        // A different stamp is a new stretch of work: the count of it
        // begins at the work itself, wherever the first frame that sees
        // it fell (see `started`).
        if pending != self.stamp {
            self.stamp = pending;
            self.started = pending.map(|since| {
                let lead = Instant::now().saturating_duration_since(since);
                now.checked_sub(lead).unwrap_or(now)
            });
        }
        // Work the user has now waited on for the threshold: the earliest
        // the bar (and its announcement) may show. Shorter work shows
        // nothing at all, and is never announced as busy either.
        let waited = self
            .started
            .map(|started| now.saturating_duration_since(started));
        let busy = waited.is_some_and(|waited| waited >= motion::LOADING_AFTER);
        // The strength the bar settles at: full while the work is waited
        // on, hidden once it is not, and under reduced motion partial —
        // still — rather than full.
        let settled = match (busy, reduced) {
            (true, true) => motion::LOADING_STILL,
            (true, false) => 1.,
            (false, _) => 0.,
        };
        let changed = settled != self.drawn;
        self.drawn = settled;
        let strength = motion::advance_loading(&mut self.fade, settled, changed, reduced, now)
            .unwrap_or(settled);
        // The sweep's pass begins with the first frame that shows the
        // line, and runs for as long as the line has any strength — the
        // one animation that runs while its cause does (see
        // `crate::ui::motion`). Reduced motion never sweeps.
        if self.swept.is_none() && strength > 0. {
            self.swept = Some(now);
        }
        let sweep = (!reduced && strength > 0.).then(|| self.sweep(now));
        let frame = LoadingFrame {
            bar: (strength > 0. || self.fade.is_some()).then(|| LoadingBar { strength, sweep }),
            animating: self.fade.is_some() || sweep.is_some(),
            busy,
            wake: match (self.started, busy) {
                // Work still beneath the threshold: one frame is due just
                // past it, and nothing else draws it.
                (Some(started), false) => wake(started),
                _ => None,
            },
        };
        #[cfg(any(test, debug_assertions))]
        {
            self.drawn_bar = frame.bar;
        }
        frame
    }

    /// The sweep's highlight along the line at `now`: its left edge and
    /// its width as shares of the line, one pass every
    /// [`LOADING_SWEEP`](crate::ui::motion). The highlight grows in at
    /// the line's left edge, travels its length and shrinks out at its
    /// right, so a pass ends with nothing on the line and the next begins
    /// clean: the sweep never jumps.
    fn sweep(&self, now: Instant) -> (f32, f32) {
        /// The highlight's width at its fullest, as a share of the line.
        const WIDTH: f32 = 0.35;
        /// The share of a pass the highlight's edges take, entering at
        /// the left and leaving at the right.
        const EDGE: f32 = 0.12;
        let swept = self.swept.unwrap_or(now);
        let pass = now.saturating_duration_since(swept).as_secs_f32()
            / motion::LOADING_SWEEP.as_secs_f32();
        let gone = pass.rem_euclid(1.);
        let (left, width) = match (gone < EDGE, gone > 1. - EDGE) {
            // Entering: the highlight grows from the left edge.
            (true, _) => (0., WIDTH * gone / EDGE),
            // Leaving: the highlight shrinks into the right edge.
            (false, true) => (1. - WIDTH * (1. - gone) / EDGE, WIDTH * (1. - gone) / EDGE),
            // Traveling: the whole highlight crosses the line.
            _ => ((gone - EDGE) / (1. - 2. * EDGE) * (1. - WIDTH), WIDTH),
        };
        (left.clamp(0., 1.), width.clamp(0., WIDTH))
    }
}

/// When the wake asks for its frame, `started` past the threshold: a
/// millisecond beyond it, so the frame it draws sees the work as having
/// outlasted the threshold — the wake's clock and the count's are the
/// same, but the lead the count began with (see [`Loading::advance`])
/// could leave the threshold a hair unmet at exactly its moment, and a
/// wake that fires without the bar showing would ask for nothing more.
fn wake(started: Instant) -> Option<Instant> {
    started.checked_add(motion::LOADING_AFTER + Duration::from_millis(1))
}

/// The loading bar along the rule under the search field: the 1px track
/// `bar` says to draw, with the sweep's highlight inside it, at the
/// strength the frame drew. The caller places it —
/// `crate::features::root_search::search_header`, for root search's field
/// and an opened command's alike (the one drawing helper) — with `sides`
/// the line's inset from its left and right edges: nothing over the plain
/// hairline, where the line spans the rule, and the frosted pill's corner
/// radius over a background image, where the pill's rounded rim leaves no
/// rule to reach (ADR 0028).
pub(crate) fn line(bar: &LoadingBar, sides: Pixels, theme: &Theme) -> Div {
    div()
        .debug_selector(|| "loading-bar".into())
        .absolute()
        .left(sides)
        .right(sides)
        .bottom_0()
        .h(px(1.))
        .opacity(bar.strength)
        .when_some(bar.sweep, |track, (left, width)| {
            // The highlight: bright at its leading edge, fading behind —
            // the head leads, the tail follows.
            track.child(
                div()
                    .debug_selector(|| "loading-sweep".into())
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(left))
                    .w(relative(width))
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(theme.warning.opacity(0.), 0.),
                        linear_color_stop(theme.warning, 1.),
                    )),
            )
        })
}

impl LauncherWindow {
    /// Wakes the window when the loading bar's threshold is due, if it
    /// waits: work still beneath the threshold needs the one frame that
    /// brings the bar (and its announcement) in, and nothing else draws
    /// it. Like the announcer's and the toast's, the wake is a timer on
    /// the executor's clock, replaced whenever the wait it serves moves
    /// and dropped when it has none.
    pub(crate) fn wake_loading(&mut self, wake: Option<Instant>, cx: &mut Context<Self>) {
        if wake != self.motion.loading.wake_at {
            self.motion.loading.wake_at = wake;
            self.motion.loading._wake = wake.map(|at| {
                let wait = at.saturating_duration_since(cx.background_executor().now());
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(wait).await;
                    this.update(cx, |_, cx| cx.notify()).ok();
                })
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A frame's worth of the count past the threshold (the wake's own
    /// margin), and a moment within the fade.
    const PAST: Duration = Duration::from_millis(301);
    const SOME: Duration = Duration::from_millis(100);

    /// A stretch of work just begun: its stamp, and when it began on the
    /// window's clock.
    fn begun(loading: &mut Loading) -> (Instant, Instant) {
        let since = Instant::now();
        loading.advance(Some(since), false, since);
        (since, loading.started.expect("the stretch began"))
    }

    #[test]
    fn work_beneath_the_threshold_shows_nothing_and_wakes_at_it() {
        let mut loading = Loading::default();
        let (since, started) = begun(&mut loading);
        let frame = loading.advance(Some(since), false, started);
        assert!(frame.bar.is_none(), "nothing shows yet");
        assert!(!frame.animating);
        assert!(!frame.busy);
        assert_eq!(frame.wake, Some(started + PAST));
        // The wait holds across frames that draw nothing.
        let frame = loading.advance(Some(since), false, started + motion::LOADING_AFTER / 2);
        assert!(frame.bar.is_none());
        assert_eq!(frame.wake, Some(started + PAST));
    }

    #[test]
    fn work_past_the_threshold_fades_in_and_sweeps() {
        let mut loading = Loading::default();
        let (since, started) = begun(&mut loading);
        let frame = loading.advance(Some(since), false, started + PAST);
        assert!(frame.busy);
        assert!(frame.animating, "the fade-in runs");
        assert_eq!(frame.wake, None, "nothing waits once the bar shows");
        let bar = frame.bar.expect("the line is drawn");
        assert_eq!(bar.strength, 0., "the fade-in begins at nothing");
        // A moment on, the line is visible and the sweep has entered.
        let frame = loading.advance(Some(since), false, started + PAST + SOME);
        let bar = frame.bar.expect("the line is drawn");
        assert!(
            bar.strength > 0.5,
            "ease-out covers most of the distance early"
        );
        assert!(bar.strength < 1., "the fade-in is not done in a moment");
        assert_eq!(bar.sweep, Some((0., 0.)), "the sweep enters from nothing");
        // The fade completes and the sweep travels; the line keeps asking
        // for frames while the work is pending.
        let at = started + PAST + motion::LOADING_FADE;
        let frame = loading.advance(Some(since), false, at);
        let bar = frame.bar.expect("the line is drawn");
        assert_eq!(bar.strength, 1., "the fade-in is done");
        assert!(frame.animating, "the sweep keeps the frames coming");
        let (left, width) = bar.sweep.expect("the sweep runs");
        assert_eq!(width, 0.35, "the highlight is at its fullest");
        assert!(
            left > 0. && left < 0.5,
            "the highlight is traveling: {left}"
        );
    }

    #[test]
    fn work_that_ends_fades_the_line_away_and_then_stops() {
        let mut loading = Loading::default();
        let (since, started) = begun(&mut loading);
        loading.advance(Some(since), false, started + PAST);
        let shown = started + PAST + SOME;
        let strength = loading
            .advance(Some(since), false, shown)
            .bar
            .expect("the line is showing")
            .strength;
        assert!(strength > 0.5 && strength < 1., "the line is on its way in");
        // The work ends: the line fades away from where it was.
        let frame = loading.advance(None, false, shown);
        assert!(!frame.busy, "the work is over");
        assert!(frame.animating, "the fade-out runs");
        let frame = loading.advance(None, false, shown + SOME);
        let fading = frame.bar.expect("the line still shows").strength;
        assert!(
            fading < strength,
            "the line is leaving: {fading} of {strength}"
        );
        let gone = shown + motion::LOADING_FADE;
        let frame = loading.advance(None, false, gone);
        assert!(frame.bar.is_none(), "the line is gone");
        assert!(!frame.animating, "no frame is asked for");
        assert!(!frame.busy);
        assert_eq!(frame.wake, None);
    }

    #[test]
    fn under_reduced_motion_the_line_is_still_at_partial_strength() {
        let mut loading = Loading::default();
        let since = Instant::now();
        loading.advance(Some(since), true, since);
        let started = loading.started.expect("the stretch began");
        let frame = loading.advance(Some(since), true, started + PAST);
        let bar = frame.bar.expect("the line is drawn at once");
        assert_eq!(bar.strength, motion::LOADING_STILL);
        assert_eq!(bar.sweep, None, "nothing sweeps");
        assert!(!frame.animating, "a still line asks for no frame");
        assert!(frame.busy, "the announcement may speak it");
        // It leaves at once when the work ends.
        let frame = loading.advance(None, true, started + PAST + SOME);
        assert!(frame.bar.is_none());
        assert!(!frame.animating);
    }

    #[test]
    fn a_new_stamp_of_work_begins_a_new_count() {
        let mut loading = Loading::default();
        let (since, started) = begun(&mut loading);
        loading.advance(Some(since), false, started + PAST);
        loading.advance(Some(since), false, started + PAST + SOME);
        // The first stretch's work is replaced by another, still young:
        // the count begins anew, and the line that was showing fades away.
        let second = Instant::now();
        let at = started + PAST + SOME;
        let frame = loading.advance(Some(second), false, at);
        assert!(!frame.busy, "the new work has just begun");
        assert!(frame.bar.is_some(), "the old line is fading away");
        let restarted = loading.started.expect("the new stretch began");
        let frame = loading.advance(Some(second), false, restarted + PAST / 2);
        assert!(!frame.busy, "the new work is still beneath the threshold");
        assert_eq!(frame.wake, Some(restarted + PAST));
        // The new work outlasts the threshold in its own count: the line
        // is back, and fully shown a fade later.
        let frame = loading.advance(Some(second), false, restarted + PAST);
        assert!(frame.busy, "the new work's count is its own");
        let frame = loading.advance(Some(second), false, restarted + PAST + motion::LOADING_FADE);
        assert_eq!(frame.bar.expect("the line is back").strength, 1.);
    }

    #[test]
    fn the_sweep_travels_the_line_and_wraps_cleanly() {
        let mut loading = Loading::default();
        let (since, started) = begun(&mut loading);
        loading.advance(Some(since), false, started + PAST);
        // Frames 100ms on, from the first that shows the line (where the
        // sweep's pass begins): 1.5s of them, past a whole pass.
        let mut last = 0.;
        for step in 1..=15 {
            let at = started + PAST + SOME * step;
            let frame = loading.advance(Some(since), false, at);
            let bar = frame.bar.expect("the line is drawn");
            let (left, width) = bar.sweep.expect("the sweep runs");
            assert!(left + width <= 1. + 1e-6, "the highlight stays on the line");
            assert!(
                width <= 0.35 + 1e-6,
                "the highlight is never wider than its width"
            );
            assert!(
                left >= last - 1e-6,
                "the highlight only moves right: {left} < {last}"
            );
            last = left;
        }
        // One pass every LOADING_SWEEP: a sweep that long after it began
        // is where it started.
        let frame = loading.advance(
            Some(since),
            false,
            started + PAST + SOME + motion::LOADING_SWEEP,
        );
        let bar = frame.bar.expect("the line is drawn");
        assert_eq!(
            bar.sweep,
            Some((0., 0.)),
            "the next pass enters from nothing"
        );
    }
}
