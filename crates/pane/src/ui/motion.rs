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
//! - A **view transition** — the launcher's screen *kind* changes going
//!   forward (root search to a command, a form or custom view opening)
//!   because the pointer opened it (a row clicked, an entry from the
//!   Settings window) — moves the content that changes: the arriving
//!   content fades in over a tiny shift from below. Every keyboard open
//!   (Enter, Ctrl and a number, a command's hotkey) lands at once, and so
//!   does backing out (Escape, return to root): keyboard opens and exits
//!   are the most frequent actions in a launcher, repeated many times a
//!   day, and neither should ever be something the user waits on. The shift is a
//!   relative `top` inset, applied after layout like a CSS transform, so
//!   the stable shell chrome (the panel, the footer with its action
//!   strip, the query field, the heading) never moves. The departing
//!   content is unmounted at once: it is never drawn fading out, so it
//!   can expose no hit targets, no active accessibility nodes, and cannot
//!   pin a departed screen or a guest runtime generation — the
//!   transition holds no screen, no rows and no callbacks at all.
//! - A **Settings section change** — the sidebar's selection moves to
//!   another page — is the same shape at Settings' scale: the page's
//!   content fades in over a tiny shift from the side the sidebar moved,
//!   while the shell around it (the sidebar, the titlebar, the page's
//!   scroll viewport) stays exactly where it was. The section itself is
//!   already switched — the selection, the focus and the sidebar's
//!   selected row were updated before the frame draws — and a page's own
//!   state (a filter, a collapsed group, an open edit) is the page's,
//!   untouched by the paint.
//! - A **disclosure** — a Shortcuts group expands or collapses — runs one
//!   tween per group: the group's *look* (0 collapsed, 1 expanded), which
//!   the header chevron's rotation and the commands' arrival both derive
//!   from, so the two cannot drift apart or run staggered rows. Expanding
//!   mounts the rows at once and fades the whole block in over the tiny
//!   shift; collapsing unmounts them at once — the departing rows are
//!   never drawn fading out — while the chevron turns back on the same
//!   timeline. Collapsing moves the focus out of the rows to the header
//!   that controls them, so nothing hidden can keep the input.
//! - A **popup** — the launcher footer's ellipsis menu, a Settings
//!   select's dropdown — is the one family that animates both ways: an
//!   entrance (140ms) that rises out of its trigger over a fade, and a
//!   shorter exit (100ms) that recedes back toward it, the way out
//!   faster than the way in for the same reason a view's return is. The
//!   shift is the same relative-inset treatment the view transitions
//!   use, on a wrapper *inside* the popup: the anchored element still
//!   measures the popup at its resting size, so the placement and the
//!   window-edge snapping never move mid-flight, and the surface, its
//!   shadow and its contents move as one — never a cascade of options.
//!   A popup is overlay chrome, not content, so its exit is allowed the
//!   one thing no content transition is: the closed popup keeps painting
//!   for its bounded exit — *inert*. The frame that closes it has
//!   already done everything the input contract asks (the draft is
//!   settled, the focus returned or handed over), the exiting visuals
//!   carry no handlers and are hidden from accessibility, and the
//!   overlay keeps occluding so a click on it cannot invoke what is
//!   underneath; the frame that completes the exit unmounts it all, so
//!   nothing of a closed popup can intercept a click. Reopening during
//!   the exit retargets the same tweens (the look and the fade) from the
//!   presentation on screen —
//!   a reversal, not a second overlay stacked on a dying one.
//! - A **query or result update** — typing, rows changing, selection,
//!   status, a screen's own contents, a Settings page's filter narrowing
//!   or clearing, a popup's own rows filtering — never animates.
//!   Navigation, dispatch, cancellation and focus are applied by the
//!   window before any frame draws; the transition only paints what
//!   already changed, so it can never rerun a command, delay its
//!   request, resubmit a form, change history or wait for typing.
//!   Backing out mid-arrival drops the arrival, so root lands settled; a
//!   rapid section switch or a re-reversed disclosure starts the next
//!   transition from the current presentation (the interrupted value), so
//!   a reversal retargets smoothly instead of flashing.
//! - A **control's pointer feedback** — the wash a row, an item or a
//!   button takes under the pointer, and the stronger wash it takes
//!   while pressed ([`crate::ui::theme::pressed`]) — changes at once:
//!   the hover tens of times a day, the press the instant the pointer
//!   goes down, so the family has no motion at all. (GPUI fades a
//!   property the same way in every state, so a hover fade would have
//!   delayed the press too.) Keyboard focus and an option's active
//!   state stay rest styles: the focus ring and the selected wash.
//! - **The loading bar** — the one-pixel line along the rule under the
//!   search field's, or an opened command's search field's, rule while
//!   waited-for work has outlasted [`LOADING_AFTER`] (#248, ADR 0035) —
//!   sweeps a soft highlight across itself, one pass every
//!   [`LOADING_SWEEP`], fading in over [`LOADING_FADE`] once the work has
//!   outlasted the threshold and out again once it ends. It is the one
//!   animation that runs for as long as its cause does: while such work
//!   is pending, the window keeps asking for frames, and it stops asking
//!   on the frame the fading bar reaches nothing. Under reduced motion
//!   the line is still, at partial strength (see [`LOADING_STILL`]).
//!
//! Reduced motion: [`App::reduce_motion`] decides, and
//! [`observe_reduced_motion`] connects that flag to what the operating
//! system actually reports — see that function for what is detected on
//! each system and what the documented fallback is there. Where the
//! system reports changes while Pane runs, a change lands on the next
//! drawn frame: engaged mid-transition, that frame settles at once and
//! schedules no further cosmetic frames.
//!
//! Frame discipline: every transition — a view transition, a Settings
//! section arrival, a group disclosure, a popup's entrance or exit —
//! runs for its bounded duration (a control's hover and pressed washes
//! have none: they change at once) and requests
//! animation frames only while one is in flight. Completing, cancelling
//! (the screen changed again), reduced motion, an unmounted window and a
//! hidden window all end with a frame that requests nothing — the window
//! is idle. Since progress is measured on a clock rather than counted in
//! frames, a window that was hidden mid-transition settles on the first
//! frame it is shown again and then stops; there is no ambient animation
//! of any kind — the loading bar's sweep is not ambient: it runs only
//! while work the user waits for is pending, and ends with it. The
//! functional scroll relayout in
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
    /// Into a view the pointer opened: a row clicked, a package previewed
    /// or a root result opened from the Settings window. The arriving
    /// content rises into place from below.
    Forward,
    /// Back out — returning to root search or a shallower view — and
    /// every change nothing armed: a keyboard open, a summon, a reply.
    /// The arriving content lands at once, with no transition.
    Back,
}

/// How long the content of a view the pointer opened takes to arrive:
/// 100ms, a hint of direction rather than a motion the user watches;
/// ease-out quint covers most of the distance in the first third of it.
/// Keyboard opens skip it entirely (see the module docs).
pub(crate) const VIEW_ENTER: Duration = Duration::from_millis(100);

/// How long the content of a Settings section takes to arrive when the
/// user switches sections: 150ms — in the ticket's 120-180ms window, the
/// same span the launcher's entrances use, because moving between
/// sections is a lateral move rather than an entrance or an exit: both
/// directions of it share the one span.
pub(crate) const SECTION_ARRIVAL: Duration = Duration::from_millis(150);

/// How long a disclosure group's expansion — and the chevron that
/// announces it — takes: 180ms, the disclosure span the motion research
/// proposes from Roboco's collapse timing, at the top of the spec's
/// 120-180ms window. Expansion and collapse share it, as the ticket's
/// coordination asks: the chevron and the content run one timeline.
pub(crate) const DISCLOSURE: Duration = Duration::from_millis(180);

/// How long a popup — the launcher footer's ellipsis menu, a Settings
/// select's dropdown — takes to appear from its trigger: 140ms, Roboco's
/// own menu entrance, in the ticket's starting window. The popup's
/// surface, its shadow and its contents arrive together over the same
/// tiny shift ([`VIEW_SHIFT`]) the view transitions use, toward rest
/// from the trigger; the first frame already shows the popup faintly,
/// from the same floor the view transitions fade from.
pub(crate) const POPUP_ENTER: Duration = Duration::from_millis(140);

/// How long a popup's exit takes: 100ms, Roboco's menu exit — the way
/// out faster than the way in, as a view's return is faster than its
/// entrance. The exit recedes toward the trigger and fades all the way
/// to nothing, so the popup unmounts invisible; see the module docs for
/// what the exit's inert visuals may and may not do while it runs.
pub(crate) const POPUP_EXIT: Duration = Duration::from_millis(100);

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

/// One tween in flight: a scalar moving from `from` toward `target`
/// along the shared ease, its progress measured on the executor's
/// clock so it is deterministic under the test platform's controlled
/// clock. The same record drives every Pane transition: the launcher's
/// view arrivals (an offset tweening to rest), a Settings section's
/// arrival (the same, from the side the sidebar moved), a disclosure
/// group's look (0 collapsed, 1 expanded), which the chevron's rotation
/// and the commands' arrival both follow, and a popup's look (0 closed,
/// 1 open), from which the popup's shift toward its trigger and its
/// fade both derive.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tween {
    /// The value the tween rests at.
    target: f32,
    /// Where the tween started, in the same units: the full distance
    /// for a fresh transition, or the interrupted presentation's value
    /// when one mid-flight was retargeted.
    from: f32,
    /// When the tween started, on the executor's clock.
    started: Instant,
    /// How long the tween runs, stretched by the measurement scale.
    duration: Duration,
}

impl Tween {
    /// The tween's value at `now`: `from` when it started, `target`
    /// once its duration has passed.
    fn value(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.started);
        if self.duration.is_zero() || elapsed >= self.duration {
            return self.target;
        }
        let progress = ease(elapsed.as_secs_f32() / self.duration.as_secs_f32());
        self.target + (self.from - self.target) * (1. - progress)
    }
}

/// The curve every view transition follows: GPUI CE's quintic ease-out,
/// fast-starting and gently-settling with no overshoot — the approved
/// shape, from the pinned renderer rather than a hand-rolled curve.
fn ease(progress: f32) -> f32 {
    (gpui::ease_out_quint())(progress.clamp(0., 1.))
}

/// Below this much distance from its target a presentation counts as
/// already settled: a retarget from there has nothing to continue and no
/// transition starts. Small enough for both units the tweens run in —
/// a px offset and a 0-1 look — that nothing visible is skipped.
const SETTLED_WITHIN: f32 = 0.001;

/// Starts the tween for a presentation that just changed.
///
/// A fresh change starts the tween the full `fresh` distance from its
/// `target`. A tween still in flight hands its *current* value to the new
/// one instead, so a rapid reversal (open, back, open; expand, collapse,
/// expand; section, section, section) continues from the presentation on
/// screen — no restart, no dip to the opacity floor, no queued sequence.
/// The departing presentation's content is already gone; this never draws
/// it again.
fn arrive(
    target: f32,
    fresh: f32,
    duration: Duration,
    interrupted: Option<Tween>,
    now: Instant,
) -> Option<Tween> {
    let from = match interrupted {
        Some(was) => was.value(now),
        None => fresh,
    };
    if (from - target).abs() < SETTLED_WITHIN {
        None
    } else {
        Some(Tween {
            target,
            from,
            started: now,
            duration: duration.mul_f32(measurement_scale()),
        })
    }
}

/// Advances a tween to the frame about to be drawn and returns its
/// value; `None` means settled — draw the endpoint and request no
/// animation frame for it.
///
/// `changed` says the presentation changed since the last drawn frame (a
/// real transition); content updates pass `false` and never animate.
/// `reduced` is [`App::reduce_motion`]: reduced motion settles immediately
/// — no tween is started or kept — and, engaged mid-transition, the very
/// next frame lands settled. A tween that has run its duration ends here,
/// so nothing keeps requesting frames once the presentation has arrived.
fn advance_tween(
    tween: &mut Option<Tween>,
    target: f32,
    fresh: f32,
    duration: Duration,
    changed: bool,
    reduced: bool,
    now: Instant,
) -> Option<f32> {
    if reduced {
        *tween = None;
    } else if changed {
        *tween = arrive(target, fresh, duration, tween.take(), now);
    }
    let in_flight = (*tween)?;
    if now.saturating_duration_since(in_flight.started) >= in_flight.duration {
        // The tween has run its course: the presentation has arrived, so
        // the record goes and no further frame is requested for it. Only
        // the clock ends a tween — ending it on closeness to the target
        // would cut the ease's long tail short and snap the rest.
        *tween = None;
        return None;
    }
    Some(in_flight.value(now))
}

/// The fade that follows an arrival offset: the floor at the full shift,
/// full opacity at rest. Deriving it from the offset is what makes a
/// retarget continuous — the interrupted presentation's opacity carries
/// over exactly, because its offset does.
fn fade(offset: f32) -> f32 {
    let settled = 1. - offset.abs() / VIEW_SHIFT;
    VIEW_OPACITY_FLOOR + (1. - VIEW_OPACITY_FLOOR) * settled
}

/// Advances the window's view-transition state to the frame about to be
/// drawn, and returns the arriving content's presentation: its offset from
/// rest in px and its opacity. `None` means settled — draw the content
/// plain and request no animation frame for it.
///
/// `screen_changed` says the launcher's screen *kind* changed since the
/// last drawn frame (a real view transition); query and result updates
/// pass `false` and never animate. Backing out lands at once: a change in
/// the [`Direction::Back`] direction drops any arrival in flight and
/// starts none. `reduced` is [`App::reduce_motion`]: reduced motion
/// settles immediately — no transition is started or kept — and, engaged
/// mid-transition, the very next frame lands settled. A transition that
/// has run its duration ends here, so nothing keeps requesting frames once
/// the content has arrived.
pub(crate) fn advance(
    transition: &mut Option<Tween>,
    navigation: Direction,
    screen_changed: bool,
    reduced: bool,
    now: Instant,
) -> Option<(f32, f32)> {
    if screen_changed && navigation == Direction::Back {
        *transition = None;
        return None;
    }
    let offset = advance_tween(
        transition,
        0.,
        VIEW_SHIFT,
        VIEW_ENTER,
        screen_changed,
        reduced,
        now,
    )?;
    Some((offset, fade(offset)))
}

/// Advances a Settings section's arrival — the same shape the launcher's
/// view transitions have, over the section span. `from` is the side the
/// content arrives from on a fresh switch: below rest when the sidebar
/// moved down to the new section, above rest when it moved up. Returns
/// the arriving content's (offset, opacity); `None` when settled, which
/// is also all reduced motion ever reports.
pub(crate) fn advance_arrival(
    arrival: &mut Option<Tween>,
    from: f32,
    changed: bool,
    reduced: bool,
    now: Instant,
) -> Option<(f32, f32)> {
    let offset = advance_tween(arrival, 0., from, SECTION_ARRIVAL, changed, reduced, now)?;
    Some((offset, fade(offset)))
}

/// Advances a disclosure group's look — 0 collapsed, 1 expanded — over
/// the disclosure span. `expanded` is the group's drawn state this frame;
/// `toggled` says the user toggled this group since the last drawn frame,
/// which is the only thing that starts or retargets the tween: a filter
/// change and a group's first draw are content updates, which never
/// animate. Returns the group's look while the disclosure is in flight;
/// `None` when settled, which draws the endpoint and requests no frame.
/// The look is the one number the group's chevron rotation and its
/// commands' arrival both derive from, so they share one timeline and
/// retarget together.
pub(crate) fn advance_disclosure(
    disclosure: &mut Option<Tween>,
    expanded: bool,
    toggled: bool,
    reduced: bool,
    now: Instant,
) -> Option<f32> {
    let target = if expanded { 1. } else { 0. };
    advance_tween(
        disclosure,
        target,
        1. - target,
        DISCLOSURE,
        toggled,
        reduced,
        now,
    )
}

/// How long the launcher's number hints take to slide in once Ctrl has
/// been held (see [`advance_reveal`]).
pub(crate) const REVEAL_IN: Duration = Duration::from_millis(160);

/// How long the number hints take to slide away when Ctrl is released:
/// 120ms, faster than they arrive, as every exit here is faster than its
/// entrance.
pub(crate) const REVEAL_OUT: Duration = Duration::from_millis(120);

/// How long waited-for work must run before the loading bar under the
/// search field's rule shows at all (#248, ADR 0035: "a loading bar that
/// flashes for an instant answer is noise"): an answer that comes
/// quickly shows none, and the busy state is announced only once it has
/// passed too. The loading module keeps the rest of the bar's policy;
/// this span is also the moment the window waits before it speaks the
/// busy state, so the two are one threshold.
pub(crate) const LOADING_AFTER: Duration = Duration::from_millis(300);

/// How long the loading bar takes to fade in once waited-for work has
/// outlasted [`LOADING_AFTER`], and to fade away once the work ends: the
/// same span either way, so the line leaves as softly as it comes.
pub(crate) const LOADING_FADE: Duration = Duration::from_millis(300);

/// How often the loading bar's soft highlight sweeps across the rule
/// under the search field: one full pass, left to right, each time.
pub(crate) const LOADING_SWEEP: Duration = Duration::from_millis(1500);

/// The loading bar's strength under reduced motion, where nothing
/// sweeps: the line shows at partial strength, still.
pub(crate) const LOADING_STILL: f32 = 0.5;

/// Advances a reveal — the number hints' look, 0 hidden, 1 shown — over
/// the reveal spans, as [`advance_disclosure`] advances a group's.
/// `shown` is the hints' state this frame and `changed` says it flipped
/// since the last drawn frame. Returns the look while the reveal is in
/// flight; `None` when settled.
pub(crate) fn advance_reveal(
    reveal: &mut Option<Tween>,
    shown: bool,
    changed: bool,
    reduced: bool,
    now: Instant,
) -> Option<f32> {
    let (target, duration) = if shown {
        (1., REVEAL_IN)
    } else {
        (0., REVEAL_OUT)
    };
    advance_tween(reveal, target, 1. - target, duration, changed, reduced, now)
}

/// Advances the loading bar's strength toward `target` — 0 hidden, 1
/// shown, [`LOADING_STILL`] under reduced motion — over the fade span,
/// as [`advance_reveal`] advances the number hints' look: `changed` says
/// the target moved since the last drawn frame. Returns the strength
/// while a fade is in flight; `None` when settled, which draws `target`
/// and requests no frame for the fade — the sweep is what keeps asking
/// while the bar is shown (see `crate::features::loading`).
pub(crate) fn advance_loading(
    fade: &mut Option<Tween>,
    target: f32,
    changed: bool,
    reduced: bool,
    now: Instant,
) -> Option<f32> {
    advance_tween(
        fade,
        target,
        1. - target,
        LOADING_FADE,
        changed,
        reduced,
        now,
    )
}

/// Advances a popup's entrance or exit — the popup's look, 0 closed, 1
/// open — over the popup family's spans, and returns its presentation
/// while one is in flight: the offset from rest toward the trigger, in
/// px, and the opacity. `None` means settled: an open popup draws at
/// rest, a closed one draws nothing at all.
///
/// `open` is the popup's *interaction* state this frame — the state the
/// input contract has already settled (a closed popup has already
/// returned or handed over its focus, discarded nothing it needs and
/// become inert). `changed` says that state flipped since the last drawn
/// frame: opening starts the entrance from the trigger, closing starts
/// the exit toward it, and a flip while a tween is still in flight — a
/// close during the entrance, a reopen during the exit — retargets from
/// the presentation on screen, so a reversal continues instead of
/// restarting and the same popup element turns around. `toward` is the
/// signed px offset from rest toward the popup's trigger: a popup that
/// hangs below its trigger passes a negative value (toward it is up),
/// one above its trigger a positive one. The exit's fade runs all the
/// way to nothing — the popup unmounts invisible — while the entrance
/// starts from the floor the view transitions fade from, so its first
/// frame already shows the popup faintly. Because the two ends differ,
/// the fade is its own tween beside the look's rather than a function of
/// the look: a reversal retargets each from what is on screen, so the
/// opacity turns around as smoothly as the shift does. Reduced motion
/// settles either way at once, as everywhere.
pub(crate) fn advance_popup(
    popup: &mut PopupMotion,
    open: bool,
    toward: f32,
    changed: bool,
    reduced: bool,
    now: Instant,
) -> Option<(f32, f32)> {
    let (target, fresh_opacity, duration) = if open {
        (1., VIEW_OPACITY_FLOOR, POPUP_ENTER)
    } else {
        (0., 1., POPUP_EXIT)
    };
    let opacity = advance_tween(
        &mut popup.fade,
        target,
        fresh_opacity,
        duration,
        changed,
        reduced,
        now,
    );
    let look = advance_tween(
        &mut popup.look,
        target,
        1. - target,
        duration,
        changed,
        reduced,
        now,
    )?;
    Some((toward * (1. - look), opacity.unwrap_or(target)))
}

/// A popup's motion in flight: its look (0 closed, 1 open), which the
/// shift toward the trigger follows, and its fade, which starts from a
/// different place each way (see [`advance_popup`]). Both run the same
/// span on the same curve, so they settle on the same frame.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PopupMotion {
    look: Option<Tween>,
    fade: Option<Tween>,
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

/// Wraps `content` — the area that changes between the Settings window's
/// sections, a page's content inside its scroll viewport — with the
/// arriving content's presentation, or plain when `arriving` is `None`
/// (settled). The same treatment as [`arriving`], minus the flex sizing
/// the launcher's panel child needs: this wrapper lives inside a
/// scrolling container, so it keeps the content's own size and the
/// viewport — shell chrome, like the sidebar and the titlebar — stays
/// steady while the content arrives. The wrapper is always in the tree,
/// with no-op styles at rest, so the wrapped page keeps its identity and
/// state across every switch; its content stays interactive and present
/// to assistive technology from the first frame, because the window has
/// already switched sections — only the paint eases in.
pub(crate) fn arriving_page(
    content: impl gpui::IntoElement,
    arriving: Option<(f32, f32)>,
) -> gpui::Div {
    let (offset, opacity) = arriving.unwrap_or((0., 1.));
    div()
        .relative()
        .top(px(offset))
        .when(opacity < 1., |wrapper| wrapper.opacity(opacity))
        .child(content)
}

/// The measurement scale (`PANE_MOTION_SCALE`, default 1): stretches every
/// transition timeline by this factor, for native frame captures that
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
