//! Pane's shared motion policy: the one place the launcher's animation
//! timings, curves and distances come from, and the rules that keep
//! animation strictly presentation-only.
//!
//! Provenance: the timings and the paired forward/back shape are adapted
//! from Roboco's pinned motion catalog
//! (`docs/research/roboco-motion-settings.md`), which the approved scope
//! extension of #70 names as the experiential reference — not as constants
//! to copy into a different solver. The starting points are the ticket's
//! own: entrances around 120-180ms, a faster return, a 2-4 logical pixel
//! shift, fast-starting and gently-settling easing, no exaggerated bounce,
//! no per-row cascade, no animated typing. The curve itself is GPUI CE's
//! pinned `ease_out_quint` (quintic ease-out), so the pinned renderer
//! stays the only animation toolkit: nothing here re-implements springs
//! or timelines. The one mechanism is the same one Roboco's native
//! helpers use for interruptible motion at their renderer revision —
//! progress measured on a clock, frames requested from the render tail —
//! because GPUI's `with_animation` clock restarts from zero whenever its
//! element identity changes, which replays an entrance mid-exit (the
//! pinned Roboco helper documents exactly that limitation).
//!
//! What animates and what never does:
//!
//! - A **view transition** — the launcher's screen *kind* changes (root
//!   search to a command, a command back to root, a form or custom view
//!   opening or closing) — moves the content that changes: the arriving
//!   content fades in over a tiny directional shift. The shift is a
//!   relative `top` inset, applied after layout like a CSS transform, so
//!   the stable shell chrome (the panel, the footer with its action
//!   strip, the query field, the heading) never moves. The departing
//!   content is unmounted at once: it is never drawn fading out, so it
//!   can expose no hit targets, no active accessibility nodes, and cannot
//!   pin a departed screen or a guest runtime generation — the
//!   transition holds no screen, no rows and no callbacks at all.
//! - A **query or result update** — typing, rows changing, selection,
//!   status, a screen's own contents — never animates. Navigation,
//!   dispatch, cancellation and focus are applied by the launcher before
//!   any frame draws; the transition only paints what already changed, so
//!   it can never rerun a command, delay its request, resubmit a form,
//!   change history or wait for typing. A rapid open/back/open starts the
//!   next transition from the current presentation (the interrupted
//!   offset), so a reversal retargets smoothly instead of flashing.
//!
//! Reduced motion: [`App::reduce_motion`] decides, and
//! [`observe_reduced_motion`] connects that flag to what the operating
//! system actually reports — see that function for what is detected on
//! each system and what the documented fallback is there. Where the
//! system reports changes while Pane runs, a change lands on the next
//! drawn frame: engaged mid-transition, that frame settles at once and
//! schedules no further cosmetic frames.
//!
//! Frame discipline: a view transition runs for its bounded duration and
//! requests animation frames only while one is in flight. Completing,
//! cancelling (the screen changed again), reduced motion, an unmounted
//! window and a hidden window all end with a frame that requests nothing —
//! the window is idle. Since progress is measured on a clock rather than
//! counted in frames, a window that was hidden mid-transition settles on
//! the first frame it is shown again and then stops; there is no ambient
//! animation of any kind. The functional scroll relayout in
//! [`crate::app::LauncherWindow::keep_selected_visible`] is untouched: it
//! keeps its own, separate request for one more frame.
//!
//! The clock is the background executor's (`App::background_executor().now()`,
//! which on native targets *is* `std::time::Instant`), not the wall clock:
//! the same choice GPUI CE's own animation and spring elements make, so
//! animation progress is deterministic under the test platform's
//! controlled clock.

use std::time::{Duration, Instant};

use gpui::{App, div, prelude::*, px};

/// Which way a view transition goes, set by the navigation that caused it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    /// Into a view: a command opens from root search, a form or custom
    /// view opens, a package is previewed. The arriving content rises into
    /// place from below.
    Forward,
    /// Back out: returning to root search or a shallower view. The
    /// arriving content settles down into place from above.
    Back,
}

/// How long the content of a view that opens takes to arrive: 150ms, in
/// the ticket's 120-180ms window and just above Roboco's 140ms menu
/// entrance. Ease-out quint covers most of the distance in the first
/// third of it, so the arrival reads fast and the settle reads gentle.
pub(crate) const VIEW_ENTER: Duration = Duration::from_millis(150);

/// How long the content of a shallower view takes to arrive when the user
/// backs out: 120ms, faster than the entrance for the same reason
/// Roboco's menu exit (100ms) is faster than its entrance (140ms) — the
/// way out should not linger.
pub(crate) const VIEW_RETURN: Duration = Duration::from_millis(120);

/// How far the arriving content starts from its resting place, in logical
/// pixels: 3px, in the ticket's 2-4px window. Far enough to read as
/// direction, near enough never to look like scrolling.
pub(crate) const VIEW_SHIFT: f32 = 3.;

/// Where the arriving content's opacity starts: 0.3, the floor Roboco's
/// menu entrance fades from, so the first frame of a transition already
/// shows the arriving content faintly instead of a blank content area
/// that pops in. The fade reaches full opacity as the content reaches
/// rest.
pub(crate) const VIEW_OPACITY_FLOOR: f32 = 0.3;

/// One view transition in flight: the arriving content is `from` pixels
/// off its resting place and decays to rest over the direction's duration.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Transition {
    /// Which navigation caused the transition; decides the duration and,
    /// for a fresh transition, which side the content arrives from.
    pub(crate) direction: Direction,
    /// When the transition started, on the executor's clock.
    started: Instant,
    /// Where the arriving content starts, in px from rest: the full shift
    /// for a fresh transition, or the offset the interrupted presentation
    /// was at when the screen changed again.
    from: f32,
}

impl Transition {
    /// The direction's duration, stretched by the measurement scale.
    fn duration(&self) -> Duration {
        let base = match self.direction {
            Direction::Forward => VIEW_ENTER,
            Direction::Back => VIEW_RETURN,
        };
        base.mul_f32(measurement_scale())
    }

    /// The content's current offset from rest, in px.
    fn offset(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.started);
        let duration = self.duration();
        if duration.is_zero() || elapsed >= duration {
            return 0.;
        }
        let progress = ease(elapsed.as_secs_f32() / duration.as_secs_f32());
        self.from * (1. - progress)
    }
}

/// The curve every view transition follows: GPUI CE's quintic ease-out,
/// fast-starting and gently-settling with no overshoot — the approved
/// shape, from the pinned renderer rather than a hand-rolled curve.
fn ease(progress: f32) -> f32 {
    (gpui::ease_out_quint())(progress.clamp(0., 1.))
}

/// Below this many px off rest an arriving view counts as already settled:
/// a retarget from there has nothing to continue and no transition starts.
const SETTLED_WITHIN: f32 = 0.05;

/// Starts the transition for a screen whose kind just changed.
///
/// A fresh navigation starts the arriving content the full shift away from
/// rest: below it, rising into place, when a view opens; above it, settling
/// down, when the user backs out. A transition still in flight hands its
/// *current* offset to the arriving content instead, so a rapid reversal
/// (open, back, open) continues from the presentation on screen — no
/// restart, no dip to the opacity floor, no queued sequence. The departing
/// screen's content is already gone; this never draws it again.
fn arrive(
    direction: Direction,
    interrupted: Option<Transition>,
    now: Instant,
) -> Option<Transition> {
    let from = match interrupted {
        Some(was) => was.offset(now),
        None => match direction {
            Direction::Forward => VIEW_SHIFT,
            Direction::Back => -VIEW_SHIFT,
        },
    };
    if from.abs() < SETTLED_WITHIN {
        None
    } else {
        Some(Transition {
            direction,
            started: now,
            from,
        })
    }
}

/// Advances the window's view-transition state to the frame about to be
/// drawn, and returns the arriving content's presentation: its offset from
/// rest in px and its opacity. `None` means settled — draw the content
/// plain and request no animation frame for it.
///
/// `screen_changed` says the launcher's screen *kind* changed since the
/// last drawn frame (a real view transition); query and result updates
/// pass `false` and never animate. `reduced` is
/// [`App::reduce_motion`]: reduced motion settles immediately — no
/// transition is started or kept — and, engaged mid-transition, the very
/// next frame lands settled. A transition that has run its duration ends
/// here, so nothing keeps requesting frames once the content has arrived.
pub(crate) fn advance(
    transition: &mut Option<Transition>,
    navigation: Direction,
    screen_changed: bool,
    reduced: bool,
    now: Instant,
) -> Option<(f32, f32)> {
    if reduced {
        *transition = None;
    } else if screen_changed {
        *transition = arrive(navigation, transition.take(), now);
    }
    let in_flight = (*transition)?;
    let offset = in_flight.offset(now);
    if offset.abs() < SETTLED_WITHIN {
        // The transition has run its course (or was retargeted from
        // nothing): the content has arrived, so the record goes and no
        // further frame is requested for it.
        *transition = None;
        return None;
    }
    // The fade follows the offset: the floor at the full shift, full
    // opacity at rest. Deriving it from the offset is what makes a
    // retarget continuous — the interrupted presentation's opacity
    // carries over exactly, because its offset does.
    let settled = 1. - offset.abs() / VIEW_SHIFT;
    let opacity = VIEW_OPACITY_FLOOR + (1. - VIEW_OPACITY_FLOOR) * settled;
    Some((offset, opacity))
}

/// Wraps `content` — the area that changes between the launcher's screens
/// (the results list, a form, a custom view) — with the arriving content's
/// presentation, or plain when `arriving` is `None` (settled). The wrapper
/// is always in the tree, with no-op styles at rest, so the wrapped
/// elements keep their identity and state across every transition.
///
/// The offset is a relative `top` inset: taffy applies relative insets
/// after layout, the way a CSS transform paints, so the shell chrome
/// around the content never moves while the content's own paint, hit
/// targets and debug bounds follow it together. Opacity is paint-only and
/// is left unset at rest; the content below stays interactive and present
/// to assistive technology from the first frame, because the launcher has
/// already navigated — only the paint eases in.
pub(crate) fn arriving(content: impl gpui::IntoElement, arriving: Option<(f32, f32)>) -> gpui::Div {
    let (offset, opacity) = arriving.unwrap_or((0., 1.));
    div()
        .relative()
        .top(px(offset))
        .when(opacity < 1., |wrapper| wrapper.opacity(opacity))
        // Stands in for the body it wraps as the flex child that fills the
        // panel between the chrome above and the footer below.
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .child(content)
}

/// The measurement scale (`PANE_MOTION_SCALE`, default 1): stretches every
/// view-transition timeline by this factor, for native frame captures that
/// need to sample a 150ms transition over a slower, observable span — the
/// same purpose Roboco's motion scale serves. Read once; clamped to
/// 0.25..16; never set by Pane itself, and a release build ignores it
/// entirely, so production timing cannot be altered through it.
#[cfg(debug_assertions)]
fn measurement_scale() -> f32 {
    static SCALE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *SCALE.get_or_init(|| {
        std::env::var("PANE_MOTION_SCALE")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|scale| scale.is_finite())
            .map(|scale| scale.clamp(0.25, 16.))
            .unwrap_or(1.)
    })
}

#[cfg(not(debug_assertions))]
fn measurement_scale() -> f32 {
    1.
}

/// Follows the operating system's reduced-motion preference for the whole
/// app: sets [`App::reduce_motion`] from the native read now, and keeps
/// following the preference where the system reports its changes (a change
/// lands on the next drawn frame — see [`advance`]). Call once, at
/// startup, before the first window opens.
///
/// What is actually detected, per system — no more is claimed:
///
/// - **Windows**: the user's animation preference, read through WinRT
///   (`UISettings.AnimationsEnabled`, the "Animation effects" setting),
///   then watched through its change event, which fires on one of the
///   system's own threads and is applied on the app's thread — the same
///   hop the launcher's global hotkey presses take (an unbounded channel
///   awaited in a task on the main thread). A read that fails fails
///   closed to reduced motion, the safer side when a preference that
///   exists cannot be known (the discipline the material layer applies to
///   its own preference reads).
/// - **macOS**: nothing yet — the pane crate links no AppKit, so no
///   `NSWorkspace` preference is read. Static fallback: full motion.
/// - **Linux**: nothing — no desktop exposes a standard reduced-motion
///   preference to a non-toolkit client, and Pane links no portal client.
///   Static fallback: full motion.
///
/// The two fallbacks differ on purpose. A failed read of a preference
/// that exists is an unknown, so Pane reduces; a platform with no read
/// wired at all falls back to its own default — full motion — which is a
/// statement about Pane, not about the user: no detection is claimed
/// where none runs. Wiring
/// `NSWorkspace.accessibilityDisplayShouldReduceMotion` (and whatever a
/// given Linux desktop exposes) is left for the settings work that
/// introduces Pane's first macOS and portal dependencies.
///
/// A development build also honors `PANE_TEST_REDUCE_MOTION`: when set,
/// the app runs with reduced motion regardless of the system's setting,
/// and the native read and watch are skipped — the native smokes use it
/// to capture the reduced-motion presentation without touching the
/// operator's own system settings. Nothing else reads it, and a release
/// build has no such hook.
pub(crate) fn observe_reduced_motion(cx: &mut App) {
    // The smokes' override, before any native read (see the docs above).
    #[cfg(debug_assertions)]
    if let Some(forced) = std::env::var_os("PANE_TEST_REDUCE_MOTION") {
        cx.set_reduce_motion(!forced.is_empty());
        return;
    }
    cx.set_reduce_motion(system_reduced_motion());
    // Windows reports changes to the preference as the user moves it; the
    // other systems have nothing to watch (see the module docs). The hop
    // is the one pane-core's changes and hotkey presses already take: an
    // unbounded channel awaited in a task on the app's thread. The event
    // says only that the setting moved, so the fresh value is read there,
    // on the app's own thread, and applied; the task holds the watch for
    // as long as Pane runs — when the task ends, dropping the watch
    // unsubscribes.
    #[cfg(target_os = "windows")]
    {
        let (report, mut changes) = tokio::sync::mpsc::unbounded_channel();
        if let Some(watch) = watch_reduced_motion(report) {
            cx.spawn(async move |cx| {
                let _watch = watch;
                while changes.recv().await.is_some() {
                    let reduced = system_reduced_motion();
                    cx.update(|cx| cx.set_reduce_motion(reduced));
                }
            })
            .detach();
        }
    }
}

/// The system's reduced-motion preference as this platform resolves it at
/// startup. Windows reads the native setting (failing closed to reduced
/// motion); the others have no read here and keep the default, full
/// motion, as documented on [`observe_reduced_motion`].
fn system_reduced_motion() -> bool {
    #[cfg(target_os = "windows")]
    {
        // "Animation effects" in Settings: false when the user turned
        // animations off, which is the request to reduce motion. A read
        // that fails is an unknown, and the safer side of an unknown is
        // reduced motion: failing closed, as the material layer does for
        // its own preference reads.
        use windows::UI::ViewManagement::UISettings;
        match UISettings::new().and_then(|settings| settings.AnimationsEnabled()) {
            Ok(animations) => !animations,
            Err(_) => true,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Starts watching the system's reduced-motion preference where the system
/// reports changes, returning the running watch (whose drop stops it), or
/// `None` where no changes are reported. Each time the setting moves, the
/// watch reports the bare event — the event itself carries no value, so the
/// fresh preference is re-read on the app's own thread when it is applied.
#[cfg(target_os = "windows")]
fn watch_reduced_motion(report: tokio::sync::mpsc::UnboundedSender<()>) -> Option<Watch> {
    use windows::Foundation::TypedEventHandler;
    use windows::UI::ViewManagement::UISettings;

    let settings = UISettings::new().ok()?;
    let handler = TypedEventHandler::new(move |_, _| {
        let _ = report.send(());
        Ok(())
    });
    let token = settings.AnimationsEnabledChanged(&handler).ok()?;
    Some(Watch { settings, token })
}

/// A running native watch for the reduced-motion preference. Dropping it
/// unsubscribes.
#[cfg(target_os = "windows")]
struct Watch {
    /// The settings object the subscription lives on, kept alive with it.
    settings: windows::UI::ViewManagement::UISettings,
    /// The event subscription's token.
    token: i64,
}

#[cfg(target_os = "windows")]
impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.settings.RemoveAnimationsEnabledChanged(self.token);
    }
}
