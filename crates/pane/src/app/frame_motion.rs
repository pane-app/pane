//! The launcher window's frame motion: the view transition, the footer
//! menu popup's entrance and exit, the number hints' slide and the hover
//! washes' exits — what
//! moves between frames, presentation only (see [`crate::ui::motion`] for
//! the whole policy).
//!
//! [`FrameMotion`] keeps the bookkeeping the window's frames share, and
//! the one rule of the view transition in one place: only a pointer open
//! (a clicked row or pin, an entry from the Settings window) arms the
//! arrival, the screen change it causes uses it up, and everything else —
//! a keyboard open, backing out, popping to root search, a window that
//! has just appeared — lands at once. Callers say which of those they are
//! ([`FrameMotion::pointer_open`], [`FrameMotion::land_at_once`],
//! [`FrameMotion::window_shown`]), and the window's render makes one call
//! per frame ([`FrameMotion::frame`]) for everything it draws in motion.

use std::mem::Discriminant;
use std::time::Instant;

use pane_core::Screen;

use crate::app::hover_wash::HoverWashes;
use crate::features::number_hints::Numbers;
use crate::ui::motion::{self, Direction};

/// What one frame draws of the motion in flight (see
/// [`FrameMotion::frame`]).
pub(crate) struct Frame {
    /// The arriving content's presentation — its offset from rest in px,
    /// below rest for a view that opens, and its opacity — or `None` when
    /// settled.
    pub(crate) arriving: Option<(f32, f32)>,
    /// The footer menu popup's presentation — the offset from rest toward
    /// the strip in px and the opacity — while its entrance or exit is in
    /// flight; `None` when settled.
    pub(crate) menu_popup: Option<(f32, f32)>,
    /// The number hints' look: 0 hidden, 1 shown.
    pub(crate) numbers: f32,
    /// Whether anything is still in flight, so another frame is needed.
    pub(crate) animating: bool,
}

/// The launcher window's frame motion (see the module docs).
pub(crate) struct FrameMotion {
    /// The view transition in flight, if any: the arriving screen's
    /// content is fading in over a tiny directional shift.
    transition: Option<motion::Tween>,
    /// Which way the last navigation went, for the next view transition's
    /// direction: an open the pointer made (a row clicked, an entry from
    /// the Settings window — a package preview, a root result) arms
    /// [`Direction::Forward`]; keyboard opens never do. The screen change
    /// it causes uses it up — that frame sets it back to
    /// [`Direction::Back`], so a later change nothing opened (a summon that
    /// pops to root search, a form submitted back to its list, a second
    /// reply from the same guest) lands at once instead of inheriting it.
    navigation: Direction,
    /// The screen *kind* the last frame drew, to tell a real view
    /// transition (the kind changed) from a query or result update (it
    /// did not — those never animate).
    drawn_screen: Option<Discriminant<Screen>>,
    /// The arriving content's presentation as the last frame drew it
    /// (see [`FrameMotion::view_transition`]). Test and debug builds
    /// only.
    #[cfg(any(test, debug_assertions))]
    arriving: Option<(f32, f32)>,
    /// The footer menu popup's entrance or exit in flight, if any: the
    /// popup's look (0 closed, 1 open) and its fade. One record serves
    /// both the open menu and the exit after it, so a reopen during the
    /// exit reverses from the presentation on screen.
    menu_transition: motion::PopupMotion,
    /// The menu item the popup's exit still shows, captured when the
    /// menu closed; the frame that completes the exit clears it, along
    /// with the popup it was painting. Read by the popup layer the
    /// footer menu module renders.
    menu_exit: Option<usize>,
    /// Whether the last drawn frame drew the menu popup open — the one
    /// thing that starts or retargets the popup's transition.
    drawn_menu: bool,
    /// The footer menu popup's presentation as the last frame drew it;
    /// `None` when the last frame drew the popup settled — at rest while
    /// open, absent while closed. Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    drawn_menu_popup: Option<(f32, f32)>,
    /// The number hints Ctrl reveals: the hold that shows them is the
    /// number hints module's, their slide is advanced here each frame.
    pub(crate) numbers: Numbers,
    /// The hover washes the launcher's surfaces take under the pointer,
    /// and the fade each leaves behind (#245; see
    /// [`crate::app::hover_wash`]): the surfaces report their hover, and
    /// their renders read the washes' strength as they draw.
    pub(crate) hover: HoverWashes,
}

impl FrameMotion {
    /// Nothing in flight and nothing armed: the window's first frame draws
    /// what it shows settled.
    pub(crate) fn new() -> FrameMotion {
        FrameMotion {
            transition: None,
            navigation: Direction::Back,
            drawn_screen: None,
            #[cfg(any(test, debug_assertions))]
            arriving: None,
            menu_transition: Default::default(),
            menu_exit: None,
            drawn_menu: false,
            #[cfg(any(test, debug_assertions))]
            drawn_menu_popup: None,
            numbers: Numbers::default(),
            hover: HoverWashes::default(),
        }
    }

    /// The pointer opened something — a row or pin clicked, an entry from
    /// the Settings window: the next screen change arrives. The click that
    /// ran it says so after the activation (which lands at once, as every
    /// keyboard open does), and the screen change it causes uses it up.
    pub(crate) fn pointer_open(&mut self) {
        self.navigation = Direction::Forward;
    }

    /// Whatever changes the screen next lands at once: a keyboard open,
    /// backing out, popping to root search, hiding — and it disarms
    /// whatever an earlier click left armed (a click that opened nothing,
    /// Settings or an app, changes no screen, so nothing used it up).
    pub(crate) fn land_at_once(&mut self) {
        self.navigation = Direction::Back;
    }

    /// The window has just appeared: it has nothing to arrive from, so its
    /// first frame draws whatever it shows settled, as the hotkey's show is
    /// itself never animated. A view that changes after that frame (a
    /// command's reply) still arrives as usual.
    pub(crate) fn window_shown(&mut self) {
        self.drawn_screen = None;
        self.transition = None;
    }

    /// The footer menu closed with `selected` the item it had selected:
    /// what the popup's exit keeps showing until the frame that completes
    /// it.
    pub(crate) fn menu_closed(&mut self, selected: usize) {
        self.menu_exit = Some(selected);
    }

    /// The menu item the popup's exit shows, while one runs.
    pub(crate) fn menu_exit(&self) -> Option<usize> {
        self.menu_exit
    }

    /// Advances everything in motion to the frame about to be drawn, at
    /// `now`, for a frame showing a screen of kind `screen` with the footer
    /// menu `menu_open` or not; `reduced` is [`gpui::App::reduce_motion`],
    /// under which everything settles at once.
    pub(crate) fn frame(
        &mut self,
        screen: Discriminant<Screen>,
        menu_open: bool,
        reduced: bool,
        now: Instant,
    ) -> Frame {
        // The number hints' look this frame: 0 hidden, 1 shown.
        let numbers = self.numbers.advance(reduced, now);
        // A view transition runs when the screen *kind* changed going
        // forward — root search to a command, a form or custom view
        // opening — and moves only the content that changed, while the
        // shell chrome (panel, footer, query field, heading) stays put.
        // Backing out lands at once, and query and result updates never
        // animate; the launcher has already navigated, dispatched and
        // focused when the first frame draws, so nothing waits on the
        // transition.
        let screen_changed = self.drawn_screen.is_some_and(|last| last != screen);
        let arriving = motion::advance(
            &mut self.transition,
            self.navigation,
            screen_changed,
            reduced,
            now,
        );
        self.drawn_screen = Some(screen);
        // The open that armed this arrival has had it: whatever changes the
        // screen next without opening anything lands at once.
        if screen_changed {
            self.navigation = Direction::Back;
        }
        #[cfg(any(test, debug_assertions))]
        {
            self.arriving = arriving;
        }
        // The footer menu popup's entrance or exit, on the same tween
        // machinery: only the menu's own open state flipping starts or
        // retargets it, so a reopen during the exit reverses from the
        // presentation on screen, and the closed menu's exit paints
        // inert (see [`crate::features::footer_menu`]). While the exit runs
        // the popup snapshot is what it paints; the frame that settles the
        // exit clears it.
        let menu_popup = motion::advance_popup(
            &mut self.menu_transition,
            menu_open,
            motion::VIEW_SHIFT,
            self.drawn_menu != menu_open,
            reduced,
            now,
        );
        self.drawn_menu = menu_open;
        #[cfg(any(test, debug_assertions))]
        {
            self.drawn_menu_popup = menu_popup;
        }
        if menu_open || menu_popup.is_none() {
            self.menu_exit = None;
        }
        // The hover washes' exits, on the same frame discipline: a fade
        // in flight keeps the frames coming; the frame that settles the
        // last one asks for none. Reduced motion settles them at once.
        let hover = self.hover.advance(reduced, now);
        // While the arriving content is still in flight, frames keep
        // coming; the frame that completes the transition asks for none,
        // so a settled window is idle. The same holds for the footer menu
        // popup's entrance or exit, the number hints' slide and the hover
        // washes' exits.
        let animating =
            arriving.is_some() || menu_popup.is_some() || self.numbers.reveal.is_some() || hover;
        Frame {
            arriving,
            menu_popup,
            numbers,
            animating,
        }
    }

    /// The view transition the last frame drew (see
    /// [`super::LauncherWindow::view_transition`]). Test and debug builds
    /// only.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn view_transition(&self) -> Option<(f32, f32)> {
        self.arriving
    }

    /// The footer menu popup's presentation as the last frame drew it (see
    /// [`super::LauncherWindow::menu_popup_presentation`]). Test and debug
    /// builds only.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn menu_popup_presentation(&self) -> Option<(f32, f32)> {
        self.drawn_menu_popup
    }
}
