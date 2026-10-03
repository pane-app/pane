//! The launcher footer's ellipsis menu: the leftmost control of the bottom
//! strip, opened by pointer or keyboard, holding Settings as its only
//! entry this milestone.
//!
//! The popup is the reference's L2 popover ([`Material::popover`]), laid
//! out above the footer strip: it overlays the results, and its events
//! cannot activate the result underneath — a mouse-down outside the
//! popup, capture phase, dismisses the menu and stops propagation, so
//! neither the row underneath nor the menu's own button (which would
//! reopen the menu) sees the click. Escape dismisses it and restores
//! focus; Tab and Shift-Tab dismiss it and continue focus traversal
//! where it left off.
//!
//! While the menu is open its list holds focus (the selected item is its
//! active descendant), so the launcher's keys — the query field's
//! editing, the list's selection — stay inert behind it.

use gpui::{
    AnyElement, App, BoxShadow, ClickEvent, Context, Div, FocusHandle, KeyBinding, MouseDownEvent,
    Role, Stateful, Subscription, Window, actions, div, prelude::*, px, relative, rgba,
};

use crate::app::LauncherWindow;
use crate::features::settings;
use crate::ui::icon::{Glyph, glyph};
use crate::ui::{self};

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
}

/// The menu's entries, in order. Settings is the only one this milestone;
/// later milestones append entries here, and the menu itself stays as it
/// is.
const ITEMS: [MenuItem; 1] = [MenuItem {
    title: "Settings",
    activate: LauncherWindow::choose_menu_settings,
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
    pub(crate) fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.menu.take() {
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
            cx.notify();
        }
    }

    fn menu_previous_item(&mut self, _: &PreviousItem, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.menu.as_mut()
            && menu.selected > 0
        {
            menu.selected -= 1;
            cx.notify();
        }
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

    /// The menu's button: the leftmost control of the footer strip. Its
    /// click opens the menu — and while the menu is open, the popup's
    /// outside-click dismissal consumes the click, so the button toggles
    /// rather than reopening.
    pub(crate) fn render_menu_button(
        &self,
        theme: &ui::theme::Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let open = self.menu.is_some();
        let icon_color = if open {
            theme.text_title
        } else {
            theme.text_muted
        };
        div()
            .id("footer-menu")
            .debug_selector(|| "footer-menu".into())
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .self_center()
            .size(px(32.))
            .rounded(theme.geometry.row_radius)
            .key_context(BUTTON_CONTEXT)
            .track_focus(&self.menu_button)
            .role(Role::Button)
            .aria_label("More actions")
            .aria_expanded(open)
            .on_action(cx.listener(Self::press_menu_button))
            .hover(|button| button.bg(theme.row_hover))
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
            .child(glyph(Glyph::Ellipsis, px(16.), icon_color))
    }

    /// The open menu's popup: the L2 popover above the footer strip,
    /// overlaying the results. It is the strip's first child, so its
    /// capture-phase dismissal runs before the button's own click
    /// tracking; the elevation shadow sits on the wrapper, which GPUI
    /// paints behind the surface's translucent fill (see
    /// [`Material::popover`]).
    pub(crate) fn render_menu_popup(
        &self,
        menu: &FooterMenu,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = ui::visuals().theme.clone();
        let material = ui::visuals().material;
        let geometry = &theme.geometry;
        let selected = menu.selected;
        let list = div()
            .id("menu")
            .debug_selector(|| "menu".into())
            .key_context(CONTEXT)
            .track_focus(&menu.focus)
            .role(Role::Menu)
            .aria_label("More actions")
            .p(px(6.))
            .min_w(px(200.))
            .on_action(cx.listener(Self::menu_next_item))
            .on_action(cx.listener(Self::menu_previous_item))
            .on_action(cx.listener(Self::menu_choose_item))
            .on_action(cx.listener(Self::menu_close))
            .on_action(cx.listener(Self::menu_close_forward))
            .on_action(cx.listener(Self::menu_close_backward))
            // A mouse-down anywhere outside the popup — on a result row,
            // the query field, the menu's own button — dismisses the menu
            // and is consumed: nothing underneath is activated, and the
            // button's click, which would reopen the menu, does not run.
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                this.close_menu(window, cx);
                cx.stop_propagation();
            }))
            .children(ITEMS.iter().enumerate().map(|(index, item)| {
                let item_selected = index == selected;
                div()
                    .id(("menu-item", index))
                    .debug_selector(move || format!("menu-item-{}", item.title))
                    .flex()
                    .items_center()
                    .min_h(geometry.row_min_height)
                    .px(geometry.row_padding_x)
                    .rounded(geometry.row_radius)
                    .cursor_pointer()
                    .text_size(theme.typography.row_title_size)
                    .font_weight(theme.typography.medium)
                    .text_color(theme.text_title)
                    .when(!item_selected, |item| {
                        item.hover(|item| item.bg(theme.row_hover))
                    })
                    .when(item_selected, |item| {
                        item.bg(theme.row_selected).aria_active_descendant()
                    })
                    .role(Role::MenuItem)
                    .aria_label(item.title)
                    .aria_selected(item_selected)
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        if let Some(item) = ITEMS.get(index) {
                            (item.activate)(this, window, cx);
                            this.close_menu(window, cx);
                        }
                    }))
                    .child(item.title)
            }));
        // The popover's bottom edge sits on the strip's top edge, however
        // tall the status message has grown the strip.
        div()
            .absolute()
            .left_0()
            .bottom(relative(1.))
            .flex_none()
            .shadow(vec![
                BoxShadow::new(px(0.), px(0.), rgba(0x000000CC)).spread_radius(px(0.5)),
                BoxShadow::new(px(0.), px(28.), rgba(0x000000BF))
                    .blur_radius(px(70.))
                    .spread_radius(px(-14.)),
            ])
            .child(material.popover(&theme, list))
            .into_any_element()
    }
}
