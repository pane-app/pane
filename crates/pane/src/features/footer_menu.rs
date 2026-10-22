//! The launcher footer's app menu: Pane's own menu, opened from the Pane
//! mark at the bottom strip's far left by pointer or keyboard, holding
//! Settings as its only entry. It is not the contextual Actions panel
//! (see [`crate::features::actions_panel`]): the reference's footer draws
//! the mark as decoration, and Pane makes it the app menu's button so
//! Settings stays reachable by mouse (a Windows/Pane adaptation, #95).
//!
//! The popup is the reference's L2 popover ([`Material::popover`]), laid
//! out above the footer strip: it overlays the results, and its events
//! cannot activate the result underneath — a mouse-down outside the
//! popup, capture phase, dismisses the menu and stops propagation, so
//! neither the row underneath nor the menu's own button (which would
//! reopen the menu) sees the click. Escape dismisses it and restores
//! focus; Tab and Shift-Tab dismiss it and continue focus traversal
//! where it left off. The popup occludes while it is on screen, so a
//! click that lands on it — on an entry, on its padding, or on the
//! fading surface of its exit — reaches nothing underneath.
//!
//! The popup's motion is the shared policy's popup family (see
//! [`crate::ui::motion`]): it enters from the strip over a tiny shift
//! and fade, and exits back toward it, faster. While the exit runs the
//! popup is inert — its list carries no focus, no roles and no handlers,
//! the whole subtree is hidden from accessibility, and the overlay
//! keeps occluding — and the frame that completes the exit unmounts it,
//! so nothing of a closed menu intercepts a click. Reopening during the
//! exit retargets the same transition from the presentation on screen.
//!
//! While the menu is open its list holds focus, so the launcher's keys —
//! the query field's editing, the list's selection — stay inert behind it.
//! The list is what assistive technology reports as focused; its selected
//! item is said by the window's announcer, "Settings, 1 of 1"
//! ([`crate::features::announcer`], #132).

use gpui::{
    AnyElement, App, BoxShadow, ClickEvent, Context, Div, FocusHandle, KeyBinding, MouseDownEvent,
    Role, Stateful, Subscription, Window, actions, div, prelude::*, px, relative,
};
use pane_core::KeyboardAction;

use crate::app::{HoverWashes, LauncherWindow, Spot};
use crate::features::announcer::{Listing, Opening, Selected, Target};
use crate::features::settings;
use crate::ui;
use crate::ui::footer;
use crate::ui::keycap::{CapStyle, key_sequence};

actions!(
    footer_menu,
    [
        NextItem,
        PreviousItem,
        ChooseItem,
        CloseMenu,
        CloseMenuForward,
        CloseMenuBackward,
        PressMenuButton
    ]
);

const CONTEXT: &str = "FooterMenu";
/// The app menu's name, its button's and its list's.
const MENU_NAME: &str = "Pane menu";
/// The menu button's own context, so its activation keys do not fall
/// through to the launcher's confirm while it is focused.
const BUTTON_CONTEXT: &str = "FooterMenuButton";

/// Registers the open menu's key bindings, which take precedence over the
/// launcher's while the menu holds focus, and the button's activation
/// keys while it is focused.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", NextItem, Some(CONTEXT)),
        KeyBinding::new("up", PreviousItem, Some(CONTEXT)),
        KeyBinding::new("enter", ChooseItem, Some(CONTEXT)),
        KeyBinding::new("escape", CloseMenu, Some(CONTEXT)),
        // Tab closes the menu and continues traversal, as dropdowns do:
        // it commits nothing.
        KeyBinding::new("tab", CloseMenuForward, Some(CONTEXT)),
        KeyBinding::new("shift-tab", CloseMenuBackward, Some(CONTEXT)),
        // The button opens with Enter or Space, like any button, without
        // the launcher's confirm also running.
        KeyBinding::new("enter", PressMenuButton, Some(BUTTON_CONTEXT)),
        KeyBinding::new("space", PressMenuButton, Some(BUTTON_CONTEXT)),
    ]);
}

/// One entry of the footer menu.
struct MenuItem {
    /// The entry's title.
    title: &'static str,
    /// What activating the entry does.
    activate: fn(&mut LauncherWindow, &mut Window, &mut Context<LauncherWindow>),
    /// The in-app navigation action whose binding the entry shows as its
    /// shortcut hint, if it has one: the hint follows the effective
    /// binding, so it teaches the key that opens the entry now.
    hint: Option<KeyboardAction>,
}

/// The menu's entries, in order. Settings is the only one this milestone;
/// later milestones append entries here, and the menu itself stays as it
/// is.
const ITEMS: [MenuItem; 1] = [MenuItem {
    title: "Settings",
    activate: LauncherWindow::choose_menu_settings,
    hint: Some(KeyboardAction::OpenSettings),
}];

/// The open footer menu: its own keyboard focus, its selected entry and
/// the focus to restore when it closes. Owned by the launcher window for
/// exactly as long as it is open.
pub(crate) struct FooterMenu {
    /// The menu list's focus, not a tab stop: the menu is opened by its
    /// button, not reached through traversal.
    focus: FocusHandle,
    /// The selected entry, an index into [`ITEMS`].
    selected: usize,
    /// What had focus when the menu opened, restored when it closes.
    restore: Option<FocusHandle>,
    /// Closes the menu when the window loses activation, as native menus
    /// do; ends with the menu.
    _deactivation: Subscription,
}

impl LauncherWindow {
    /// The menu's Settings entry: opens or focuses the Settings window,
    /// exactly as the root result and the local shortcut do.
    fn choose_menu_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        settings::open(&self.launcher, cx);
    }

    /// The menu button's activation: opens the menu, as its click and its
    /// keys do.
    fn press_menu_button(
        &mut self,
        _: &PressMenuButton,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_menu(window, cx);
    }

    /// Opens the footer menu, moving focus into it and remembering what
    /// had focus, to be restored when the menu closes.
    fn open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_some() {
            return;
        }
        let focus = cx.focus_handle().tab_stop(false);
        let restore = window.focused(cx);
        window.focus(&focus, cx);
        let deactivation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.close_menu(window, cx);
            }
        });
        self.menu = Some(FooterMenu {
            focus,
            selected: 0,
            restore,
            _deactivation: deactivation,
        });
        cx.notify();
    }

    /// Closes the open footer menu, if any, restoring the focus it took.
    /// The popup's exit starts on the frame this draws — the item the
    /// menu had selected is what the exit's inert visuals keep showing —
    /// and the frame that completes it unmounts them.
    pub(crate) fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.menu.take() {
            self.motion.menu_closed(menu.selected);
            if let Some(restore) = menu.restore {
                window.focus(&restore, cx);
            }
            cx.notify();
        }
    }

    fn menu_next_item(&mut self, _: &NextItem, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.menu.as_mut()
            && menu.selected + 1 < ITEMS.len()
        {
            menu.selected += 1;
            self.announcer.user_moved();
            cx.notify();
        }
    }

    fn menu_previous_item(&mut self, _: &PreviousItem, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.menu.as_mut()
            && menu.selected > 0
        {
            menu.selected -= 1;
            self.announcer.user_moved();
            cx.notify();
        }
    }

    /// The open menu as the window's announcer follows it (#132): over the
    /// screen, saying only its selected item as it opens, since the screen
    /// reader reads the menu's name as the menu takes the focus.
    pub(crate) fn menu_listing(&self) -> Option<Listing> {
        let menu = self.menu.as_ref()?;
        let item = ITEMS.get(menu.selected)?;
        Some(Listing {
            over: true,
            key: MENU_NAME.to_owned(),
            opening: Opening::Selection,
            count: ITEMS.len(),
            target: Target::Row(Selected {
                id: item.title.to_owned(),
                title: item.title.to_owned(),
                position: menu.selected + 1,
                unavailable: false,
                section: None,
            }),
            query: None,
            settled: true,
        })
    }

    /// Activates the menu's selected entry, closing the menu.
    fn menu_choose_item(&mut self, _: &ChooseItem, window: &mut Window, cx: &mut Context<Self>) {
        let activate = self
            .menu
            .as_ref()
            .and_then(|menu| ITEMS.get(menu.selected))
            .map(|item| item.activate);
        if let Some(activate) = activate {
            activate(self, window, cx);
            self.close_menu(window, cx);
        }
    }

    fn menu_close(&mut self, _: &CloseMenu, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(window, cx);
    }

    /// Closes the menu and continues focus traversal forward, as Tab does
    /// without a menu open.
    fn menu_close_forward(
        &mut self,
        _: &CloseMenuForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_menu(window, cx);
        window.focus_next(cx);
    }

    fn menu_close_backward(
        &mut self,
        _: &CloseMenuBackward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_menu(window, cx);
        window.focus_prev(cx);
    }

    /// The open menu's button: the Pane mark, the leftmost control of the
    /// footer strip, drawn at rest exactly as the reference's mark (its
    /// 18px box at the strip's 16px padding; the hover and focus chrome
    /// bleed 5px around it). Its click opens the menu — and while the menu
    /// is open, the popup's outside-click dismissal consumes the click, so
    /// the button toggles rather than reopening.
    pub(crate) fn render_menu_button(
        &self,
        theme: &ui::theme::Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let open = self.menu.is_some();
        // The button's hover wash, read as it is drawn and reported by the
        // button itself (#245): it fades out once the pointer leaves.
        let look = self
            .motion
            .hover
            .look(Spot::Button("footer-menu"), cx.background_executor().now());
        footer::mark_button(theme)
            .id("footer-menu")
            .debug_selector(|| "footer-menu".into())
            .when(open, |button| button.bg(theme.hover_wash))
            .key_context(BUTTON_CONTEXT)
            .track_focus(&self.menu_button)
            .role(Role::Button)
            .aria_label(MENU_NAME)
            .aria_expanded(open)
            .on_action(cx.listener(Self::press_menu_button))
            // The footer buttons' washes (`.fbtn`): the hover one, fading
            // out once the pointer leaves (#245), and the open one while
            // pressed.
            .when(look > 0., |button| {
                button.bg(ui::theme::faded(theme.hover_wash, look))
            })
            .active(|button| button.bg(theme.footer_button_open))
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                this.motion
                    .hover
                    .set(Spot::Button("footer-menu"), *over, cx);
            }))
            // Visible keyboard focus, the list's focus ring treatment.
            .focus(|button| {
                button.shadow(vec![
                    BoxShadow::new(px(0.), px(0.), theme.focus_ring)
                        .spread_radius(px(1.))
                        .inset(),
                ])
            })
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.open_menu(window, cx);
            }))
    }

    /// The menu popup the footer strip carries: the open menu's popup,
    /// or the exit it is still painting — `None` once the exit has
    /// settled, when nothing of the menu is on screen at all. `in_flight`
    /// is the popup's presentation while its entrance or exit runs (the
    /// offset from rest toward the strip, and the opacity), `None` at
    /// rest; see [`crate::ui::motion`] for the family's rules.
    pub(crate) fn render_menu_popup_layer(
        &self,
        in_flight: Option<(f32, f32)>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let visuals = crate::settings::launcher_visuals(cx);
        match self.menu.as_ref() {
            Some(menu) => Some(menu_popup(
                menu_list(
                    Some(&menu.focus),
                    menu.selected,
                    &self.motion.hover,
                    &visuals.theme,
                    cx,
                ),
                in_flight,
                &visuals.theme,
                visuals.material,
            )),
            // The exit paints only while its transition is in flight,
            // from the item the menu had selected when it closed — the
            // frame that settles it unmounts everything.
            None => in_flight.map(|in_flight| {
                menu_popup(
                    menu_list(
                        None,
                        self.motion.menu_exit().unwrap_or(0),
                        &self.motion.hover,
                        &visuals.theme,
                        cx,
                    ),
                    Some(in_flight),
                    &visuals.theme,
                    visuals.material,
                )
            }),
        }
    }
}

/// The menu's list of entries, interactive while the menu is open and
/// inert while its exit paints. `focus` is the open menu's focus — the
/// list's own, which holds the keyboard while the menu is open — and
/// `selected` the entry the list shows as selected; an exit passes no
/// focus, and the item the menu had selected when it closed. Without a
/// focus the list is inert: no key context, no roles, no handlers and
/// nothing in the accessibility tree, so the fading visuals expose
/// nothing active (the frame that closed the menu has already restored
/// the focus it took).
fn menu_list(
    focus: Option<&FocusHandle>,
    selected: usize,
    hover: &HoverWashes,
    theme: &ui::theme::Theme,
    cx: &mut Context<LauncherWindow>,
) -> Stateful<Div> {
    let geometry = &theme.geometry.actions;
    let now = cx.background_executor().now();
    let list = div()
        .id("menu")
        .debug_selector(|| "menu".into())
        .flex()
        .flex_col()
        .gap(geometry.list_gap)
        .p(geometry.list_padding)
        .min_w(px(200.));
    let inert = focus.is_none();
    // The interactive list: the open menu's focus, semantics and
    // handlers — its key context over the window's, the focus that stays
    // on it (the announcer says the selected entry, #132), and the outside
    // dismissal that consumes the click so nothing underneath is
    // activated.
    let list = match focus {
        Some(focus) => list
            .key_context(CONTEXT)
            .track_focus(focus)
            .role(Role::Menu)
            .aria_label(MENU_NAME)
            .on_action(cx.listener(LauncherWindow::menu_next_item))
            .on_action(cx.listener(LauncherWindow::menu_previous_item))
            .on_action(cx.listener(LauncherWindow::menu_choose_item))
            .on_action(cx.listener(LauncherWindow::menu_close))
            .on_action(cx.listener(LauncherWindow::menu_close_forward))
            .on_action(cx.listener(LauncherWindow::menu_close_backward))
            // A mouse-down anywhere outside the popup — on a result row,
            // the query field, the menu's own button — dismisses the menu
            // and is consumed: nothing underneath is activated, and the
            // button's click, which would reopen the menu, does not run.
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                this.close_menu(window, cx);
                cx.stop_propagation();
            })),
        None => list.aria_hidden(),
    };
    list.children(ITEMS.iter().enumerate().map(|(index, item)| {
        let item_selected = index == selected;
        // The entry's hover wash strength as it is drawn: the menu's
        // entries select with the keyboard and the click, never under a
        // moving pointer, so an unselected entry under the pointer takes
        // the fainter wash, fading out once it leaves (#245).
        let look = hover.look(Spot::MenuItem(index), now);
        // The entry's shortcut hint, when it has one: the binding in
        // force for the action that opens it, in the shared keycap
        // chrome.
        let hint = item.hint.map(|action| {
            crate::settings::shared(cx)
                .read(cx)
                .keyboard()
                .binding(action)
                .clone()
        });
        div()
            .id(("menu-item", index))
            .debug_selector(move || format!("menu-item-{}", item.title))
            // The Actions panel's entries (`.arow`): one family for the
            // footer's two menus.
            .flex()
            .items_center()
            .gap(geometry.row_gap)
            .h(geometry.row_height)
            .px(geometry.row_padding_x)
            .rounded(geometry.row_radius)
            .text_size(theme.typography.action_size)
            .font_weight(theme.typography.action_weight)
            .text_color(theme.action_text)
            .when(!inert, |item| item.cursor_pointer())
            .when(!inert && !item_selected && look > 0., |item| {
                item.bg(ui::theme::faded(theme.hover_wash, look))
            })
            .when(!inert, |item| {
                item.on_hover(cx.listener(move |this, over: &bool, _, cx| {
                    this.motion.hover.set(Spot::MenuItem(index), *over, cx);
                }))
            })
            // While held, a live entry takes the stronger wash of its
            // hover, or of its selected wash, at once.
            .when(!inert, |item| {
                let press = ui::theme::pressed(if item_selected {
                    theme.selection_wash
                } else {
                    theme.hover_wash
                });
                item.active(move |item| item.bg(press))
            })
            .when(item_selected, |item| item.bg(theme.selection_wash))
            .when(!inert, |entry| {
                entry
                    .role(Role::MenuItem)
                    .aria_label(item.title)
                    .aria_selected(item_selected)
                    .aria_position_in_set(index + 1)
                    .aria_size_of_set(ITEMS.len())
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        if let Some(item) = ITEMS.get(index) {
                            (item.activate)(this, window, cx);
                            this.close_menu(window, cx);
                        }
                    }))
            })
            .child(item.title)
            .when_some(hint, |item, hint| {
                item.child(div().flex_1().min_w(px(0.))).child(key_sequence(
                    &crate::keyboard::binding_keys(&hint),
                    CapStyle::Regular,
                    theme,
                ))
            })
    }))
}

/// The menu popup's chrome: the L2 popover above the footer strip — its
/// bottom edge sits on the strip's top edge, however tall the status
/// message has grown the strip — with the motion wrapper inside. The
/// wrapper is always in the tree while the popup paints, carrying the
/// entrance/exit's shift toward the strip and its fade as no-op styles
/// at rest, and the elevation shadow, so the shadow follows the surface
/// it belongs to; the shift is the same relative-inset treatment the
/// view transitions use, applied after layout. The popup occludes while
/// it is on screen — open or exiting — so a click that lands on it
/// reaches nothing underneath, and the frame that completes the exit
/// unmounts it, so no invisible overlay survives to intercept one.
fn menu_popup(
    list: Stateful<Div>,
    in_flight: Option<(f32, f32)>,
    theme: &ui::theme::Theme,
    material: ui::material::Material,
) -> AnyElement {
    let (offset, opacity) = in_flight.unwrap_or((0., 1.));
    div()
        .id("menu-popup")
        .absolute()
        // The Actions panel's insets, mirrored: 10px in from the window's
        // left edge, 8px above the strip.
        .left(theme.geometry.actions.inset)
        .bottom(relative(1.))
        .pb(theme.geometry.actions.above_footer)
        .flex_none()
        .occlude()
        .child(
            div()
                .relative()
                .top(px(offset))
                .when(opacity < 1., |wrapper| wrapper.opacity(opacity))
                .shadow(ui::material::popover_shadows(theme))
                .child(material.popover(theme, list)),
        )
        .into_any_element()
}
