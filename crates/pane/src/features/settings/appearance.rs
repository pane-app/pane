//! The Appearance page: the theme and material choices, with a live
//! preview of what both windows render.
//!
//! Every value it shows and every choice it takes goes through the host
//! settings ([`crate::settings`]) — the one entity the launcher window and
//! the Settings window both observe — so a choice made here repaints both
//! windows at once, without a restart, and is written to the record off
//! the window's thread. What the page itself owns is presentation only:
//! the choice rows, the preview's composition, and the honesty notes
//! (glass's platform fallback, an override in force, a save that failed).
//!
//! The preview is the launcher's own composition at its real scale — the
//! panel surface, a search header, one selected row, the footer strip with
//! its action and keycap — rendered with the theme and material in
//! effect, so a choice shows in it exactly as it shows in the windows.
//! The page deliberately shows both truths the material holds: the rows
//! carry the *preference*, the preview and the note under the material
//! group carry what is actually in effect — glass where the platform
//! provides frost, the solid surface (with the reason) where it does not.

use gpui::{
    AnyElement, App, BoxShadow, Context, Div, Role, ScrollAnchor, Stateful, Toggled, Window, div,
    prelude::*, px,
};
use pane_core::{Launcher, MaterialPreference, ThemePreference};

use super::{Page, SettingsWindow, search};
use crate::ui::icon::{Glyph, IconTone, glyph};
use crate::ui::keycap;
use crate::ui::material::Material;
use crate::ui::result_row::{RowContent, result_row};
use crate::ui::theme::Theme;

/// The theme choices the page offers, in row order: the preference, the
/// row's name and subtitle, and its test selector.
const THEMES: [(ThemePreference, &str, &str, &str); 3] = [
    (
        ThemePreference::System,
        "System",
        "Follow the system's light or dark appearance",
        "appearance-theme-System",
    ),
    (
        ThemePreference::Light,
        "Light",
        "Pane's light palette, whatever the system's is",
        "appearance-theme-Light",
    ),
    (
        ThemePreference::Dark,
        "Dark",
        "Pane's dark palette, whatever the system's is",
        "appearance-theme-Dark",
    ),
];

/// The material choices the page offers, in row order: the preference,
/// the row's name and subtitle, and its test selector.
const MATERIALS: [(MaterialPreference, &str, &str, &str); 2] = [
    (
        MaterialPreference::Glass,
        "Glass",
        "A translucent panel over the window's blur, where the platform provides it",
        "appearance-material-Glass",
    ),
    (
        MaterialPreference::Solid,
        "Solid",
        "An opaque window with a solid panel, the same everywhere",
        "appearance-material-Solid",
    ),
];

/// The Appearance page, registered first in the window's page list: the
/// one page of this milestone's Settings whose choices change both
/// windows as they are made.
pub(crate) fn page() -> Page {
    Page {
        title: "Appearance",
        about: "Theme and material choices, with a live preview",
        icon: (IconTone::Command, Glyph::Theme),
        render,
        search: entries,
        focus,
    }
}

/// The settings the page offers the sidebar's search: each choice of the
/// theme and material groups, named as the page names it, in the group
/// it sits in. An override in force leaves the choices listed — the page
/// still shows them — but says why none can be used here, as the page's
/// own notice does.
fn entries(_launcher: &Launcher, cx: &App) -> Vec<search::Entry> {
    let overrides = crate::settings::shared(cx).read(cx).override_descriptions();
    let unavailable = (!overrides.is_empty()).then(|| {
        let verbs = if overrides.len() == 1 {
            "overrides"
        } else {
            "override"
        };
        format!(
            "{} {} the saved choice for this process: choosing here changes nothing, and Pane \
             does not save it",
            overrides.join(" and "),
            verbs,
        )
    });
    THEMES
        .iter()
        .map(|&(_, name, _, selector)| (name, selector, "Theme"))
        .chain(
            MATERIALS
                .iter()
                .map(|&(_, name, _, selector)| (name, selector, "Material")),
        )
        .map(|(name, selector, group)| search::Entry {
            control: Some(selector.into()),
            title: name.into(),
            group: Some(group.into()),
            unavailable: unavailable.clone(),
        })
        .collect()
}

/// The page's controls take no keyboard focus (they are chosen with the
/// pointer, as the reference's settings rows are), so a jump to one
/// reveals it where it drew and the sidebar keeps the focus: `false`.
fn focus(_: &mut SettingsWindow, _: &str, _: &mut Window, _: &mut Context<SettingsWindow>) -> bool {
    false
}

/// Draws the Appearance page: the theme group, the material group with
/// its honesty note, the live preview, and whatever the host settings
/// report — an override in force, or a save that failed.
fn render(
    this: &mut SettingsWindow,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    // Everything the page shows comes from the host settings; what the
    // entity holds now is what both windows render.
    let settings = crate::settings::shared(cx);
    let (theme_preference, material_preference, overrides, status) = {
        let state = settings.read(cx);
        (
            state.theme_preference(),
            state.material_preference(),
            state.override_descriptions(),
            state.status(),
        )
    };
    let visuals = crate::settings::visuals(cx);
    let theme = &visuals.theme;
    let typography = &theme.typography;
    // A choice is offered — clickable, and saved — only where nothing
    // overrides it for this process; an override is in force otherwise,
    // and the notice below says so.
    let overridden = !overrides.is_empty();

    let page = div()
        .id("appearance")
        .debug_selector(|| "appearance".into())
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .id("appearance-title")
                .debug_selector(|| "appearance-title".into())
                .pb(px(8.))
                .text_size(typography.search_size)
                .font_weight(typography.medium)
                .text_color(theme.text_title)
                .child("Appearance"),
        )
        .when_some(
            overridden.then(|| override_notice(&overrides, theme)),
            |page, notice| page.child(notice),
        )
        .child(group(
            "Theme",
            THEMES
                .iter()
                .map(|&(preference, name, subtitle, selector)| {
                    // The choice's scroll anchor, which the search's
                    // reveal scrolls to (see the window's render).
                    let anchor = this.search_anchor(selector);
                    choice(
                        selector,
                        name,
                        subtitle,
                        preference == theme_preference,
                        !overridden,
                        anchor,
                        theme,
                        cx.listener(move |_, _, _, cx| {
                            crate::settings::shared(cx)
                                .update(cx, |settings, cx| settings.set_theme(preference, cx));
                        }),
                    )
                })
                .collect(),
            theme,
        ))
        .child(group(
            "Material",
            MATERIALS
                .iter()
                .map(|&(preference, name, subtitle, selector)| {
                    let anchor = this.search_anchor(selector);
                    choice(
                        selector,
                        name,
                        subtitle,
                        preference == material_preference,
                        !overridden,
                        anchor,
                        theme,
                        cx.listener(move |_, _, _, cx| {
                            crate::settings::shared(cx)
                                .update(cx, |settings, cx| settings.set_material(preference, cx));
                        }),
                    )
                })
                .collect(),
            theme,
        ))
        // The material's honesty note: where glass is preferred but the
        // platform normalizes it away, the reason; where a glass request
        // stands, the standing caveat that a request is not proof of
        // blur. Nothing shows where the solid surface is chosen — there
        // is nothing to explain.
        .when_some(material_note(material_preference, theme), |page, note| {
            page.child(note)
        })
        .child(
            div()
                .pt(px(6.))
                .pb(px(2.))
                .text_size(typography.row_kind_size)
                .font_weight(typography.medium)
                .text_color(theme.text_muted)
                .child("Preview"),
        )
        .child(
            div()
                .id("appearance-preview")
                .debug_selector(|| "appearance-preview".into())
                .flex_none()
                .w_full()
                .h(px(170.))
                .child(preview(theme, visuals.material)),
        )
        .when_some(status, |page, status| {
            page.child(
                div()
                    .id("appearance-status")
                    .debug_selector(|| "appearance-status".into())
                    .pt(px(10.))
                    .role(Role::Status)
                    .aria_label(status.clone())
                    .text_size(typography.row_subtitle_size)
                    .text_color(theme.danger)
                    .child(status),
            )
        });
    page.into_any_element()
}

/// The notice that an override is in force: which environment variables
/// override what, and that nothing chosen here applies or is saved. The
/// rows below show the overridden choices, disabled.
fn override_notice(overrides: &[String], theme: &Theme) -> Stateful<Div> {
    let verbs = if overrides.len() == 1 {
        "overrides"
    } else {
        "override"
    };
    let text = format!(
        "{} {} the saved choice for this process: choosing here changes nothing, and Pane \
         does not save it.",
        overrides.join(" and "),
        verbs,
    );
    div()
        .id("appearance-override")
        .debug_selector(|| "appearance-override".into())
        .pt(px(2.))
        .pb(px(8.))
        .role(Role::Status)
        .aria_label(text.clone())
        .text_size(theme.typography.row_subtitle_size)
        .text_color(theme.warning)
        .child(text)
}

/// One choice group: its label (the section label style) and its rows,
/// with the radio group's semantics.
fn group(label: &'static str, rows: Vec<Stateful<Div>>, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .pb(px(4.))
                .text_size(theme.typography.row_kind_size)
                .font_weight(theme.typography.medium)
                .text_color(theme.text_muted)
                .child(label),
        )
        .children(rows)
}

/// One choice row: the reference's row chrome carrying a radio's marks
/// and semantics. `chosen` is whether the row's choice is the one in
/// effect; `offered` is whether choosing it does anything (nothing is
/// offered while an override is in force, and the row says so by its
/// state); `anchor` is the scroll anchor the search's reveal scrolls to;
/// `on_click` reports the choice to the host settings, which repaints
/// both windows and saves.
#[allow(clippy::too_many_arguments)]
fn choice(
    selector: &'static str,
    name: &'static str,
    subtitle: &'static str,
    chosen: bool,
    offered: bool,
    anchor: ScrollAnchor,
    theme: &Theme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let typography = &theme.typography;
    let geometry = &theme.geometry;
    let row = div()
        .flex()
        .items_center()
        .gap(geometry.row_gap)
        .min_h(geometry.row_min_height)
        .px(geometry.row_padding_x)
        .rounded(geometry.row_radius)
        .when(offered, |row| row.cursor_pointer())
        .when(!offered, |row| row.opacity(0.5).cursor_default())
        .when(chosen, |row| {
            row.bg(theme.row_selected).shadow(vec![
                BoxShadow::new(px(0.), px(0.), theme.row_selected_border)
                    .spread_radius(px(1.))
                    .inset(),
            ])
        })
        .child(
            // The radio's mark, as the extension form's choices render it.
            div()
                .flex_none()
                .w(px(18.))
                .text_size(typography.row_title_size)
                .text_color(theme.text_title)
                .child(if chosen { "◉" } else { "○" }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(typography.row_title_size)
                        .font_weight(typography.medium)
                        .text_color(theme.text_title)
                        .child(name),
                )
                .child(
                    div()
                        .text_size(typography.row_subtitle_size)
                        .text_color(theme.text_muted)
                        .child(subtitle),
                ),
        );
    row.id(name)
        // The pointer feedback, on the named row: the hover wash fades
        // over the shared pointer span, and the press takes the selected
        // wash — the wash the row keeps once it is chosen, so the press
        // hands over to the choice without a jump. The fade attaches only
        // while the row is unchosen, so the chosen wash both arrives and
        // leaves at once, and only the pointer's own wash fades.
        .when(offered && !chosen, |row| {
            row.hover(|row| row.bg(theme.row_hover))
                .active(|row| row.bg(theme.row_selected))
                .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
        })
        .debug_selector(move || selector.into())
        .anchor_scroll(Some(anchor))
        .role(Role::RadioButton)
        .aria_label(name)
        .aria_toggled(if chosen {
            Toggled::True
        } else {
            Toggled::False
        })
        .when(!offered, |row| row.aria_disabled(true))
        .when(offered, |row| row.on_click(on_click))
}

/// The material's note, if the chosen material needs one: the platform's
/// reason where glass normalizes to the solid surface, or the standing
/// caveat where a glass request stands. `None` for the solid surface.
fn material_note(preference: MaterialPreference, theme: &Theme) -> Option<Stateful<Div>> {
    if preference != MaterialPreference::Glass {
        return None;
    }
    let (text, color) = match crate::ui::material::glass_fallback_reason() {
        Some(reason) => (
            format!(
                "Glass is unavailable here — {reason}. Pane renders the solid surface instead, \
                 and saves the glass choice for a system that provides it."
            ),
            theme.warning,
        ),
        None => (
            "A glass request is not proof of blur: Pane cannot see whether the compositor \
             frosted the window, and the tint stands on its own if it did not."
                .into(),
            theme.text_muted,
        ),
    };
    Some(
        div()
            .id("appearance-material-note")
            .debug_selector(|| "appearance-material-note".into())
            .pt(px(6.))
            .role(Role::Status)
            .aria_label(text.clone())
            .text_size(theme.typography.row_kind_size)
            .text_color(color)
            .child(text),
    )
}

/// The live preview: the launcher's own composition at its real scale,
/// drawn with the theme and material in effect — the panel surface, a
/// search header, one selected row, and the footer strip with its action
/// and keycap. No control in it does anything; it is a picture of what
/// the windows will show.
fn preview(theme: &Theme, material: Material) -> Div {
    let geometry = &theme.geometry;
    let content = div()
        .size_full()
        .flex()
        .flex_col()
        .child(
            // The search header: the magnifier and the placeholder the
            // launcher's own field shows.
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(geometry.search_gap)
                .px(geometry.search_padding_x)
                .h(geometry.search_height)
                .child(glyph(Glyph::Search, px(16.), theme.text_muted))
                .child(
                    div()
                        .text_size(theme.typography.search_size)
                        .text_color(theme.text_placeholder)
                        .child("Search"),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_col()
                .px(geometry.row_padding_x)
                .py(px(4.))
                .child(result_row(
                    RowContent {
                        title: "Calculator".into(),
                        subtitle: Some("A command that answers from the query".into()),
                        unavailable_reason: None,
                        unavailable_id: "preview-unavailable".into(),
                        selected: true,
                        icon: Some((IconTone::Term, Glyph::Prompt)),
                    },
                    theme,
                )),
        )
        .child(
            // The footer strip: the idle action — its label and Enter
            // keycap — right-aligned, as the launcher's strip holds them.
            Material::footer(theme).child(
                div()
                    .flex()
                    .w_full()
                    .min_w(px(0.))
                    .flex_1()
                    .min_h(px(0.))
                    .items_center()
                    .gap(geometry.action_gap)
                    .child(div().flex_1().min_w(px(0.)))
                    .child(
                        div()
                            .text_size(theme.typography.footer_size)
                            .font_weight(theme.typography.medium)
                            .text_color(theme.text_title)
                            .child("Open"),
                    )
                    .child(keycap::keycap(keycap::Key::Enter, theme)),
            ),
        );
    material.panel(theme, content)
}
