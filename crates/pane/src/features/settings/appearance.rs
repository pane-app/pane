//! The Appearance page: the theme and material choices, with a live
//! preview of what both windows render (#98).
//!
//! Every value it shows and every choice it takes goes through the host
//! settings ([`crate::settings`]) — the one entity the launcher window and
//! the Settings window both observe — so a choice made here repaints both
//! windows at once, without a restart, and is written to the record off
//! the window's thread. What the page itself owns is presentation only:
//! the controls, the preview's composition, and the honesty notes (glass's
//! platform fallback, an override in force, a save that failed).
//!
//! The page is the reference Settings board's Appearance page, through
//! the shared control families ([`crate::ui::controls`]): its controls
//! column holds a field group per setting — the label, the segmented
//! choice, the description — and the preview column the board's stage and
//! miniature launcher ([`crate::ui::preview`]), drawn with the theme, the
//! material and the bindings in effect. It offers only the settings Pane
//! has (#100): the theme (System, Light, Dark — the board shows no theme
//! choice; this one is Pane's content in the board's family) and the
//! material (Glass and Solid; the board's Frost is not a Pane material).
//! The board's accent swatches, blur and tint sliders, density and
//! toggles are deferred, and appear only in the visual workbench's
//! reference fixture.
//!
//! The page deliberately shows both truths the material holds: the
//! segments carry the *preference*, the preview and the description under
//! the material carry what is actually in effect — glass where the
//! platform provides frost, the solid surface (with the reason) where it
//! does not.
//!
//! The segments are the keyboard's too: each is a tab stop, and Enter or
//! Space chooses it as a click does, under Pane's focus ring (the board
//! draws none). While an override is in force they are drawn at the
//! board's disabled opacity, and neither take the keyboard nor answer a
//! click.

use std::collections::HashMap;

use gpui::{
    AnyElement, App, Context, Div, FocusHandle, KeyBinding, Role, ScrollAnchor, SharedString,
    Stateful, Toggled, Window, actions, div, prelude::*,
};
use pane_core::{KeyboardAction, Launcher, MaterialPreference, ThemePreference};

use super::{Page, SettingsWindow, search};
use crate::ui::controls;
use crate::ui::icon::{Glyph, IconTone};
use crate::ui::keycap::KeySequence;
use crate::ui::material::Material;
use crate::ui::preview::{self, PreviewContent, PreviewRow};
use crate::ui::settings_shell;
use crate::ui::theme::Theme;

actions!(appearance, [Choose]);

/// The key context a segment carries while it has the keyboard.
const CHOICE: &str = "AppearanceChoice";

/// Registers the segments' keys: a segment is a button of a radio group,
/// so Enter and Space choose it, as a click does.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Choose, Some(CHOICE)),
        KeyBinding::new("space", Choose, Some(CHOICE)),
    ]);
}

/// The theme choices the page offers, in segment order: the preference,
/// the segment's name, the description shown while it is chosen, and its
/// test selector.
pub(crate) const THEMES: [(ThemePreference, &str, &str, &str); 3] = [
    (
        ThemePreference::System,
        "System",
        "Follows the system's light or dark appearance, and changes with it.",
        "appearance-theme-System",
    ),
    (
        ThemePreference::Light,
        "Light",
        "Pane's light palette, whatever the system's appearance is.",
        "appearance-theme-Light",
    ),
    (
        ThemePreference::Dark,
        "Dark",
        "Pane's dark palette, whatever the system's appearance is.",
        "appearance-theme-Dark",
    ),
];

/// The material choices the page offers, in segment order: the preference,
/// the segment's name, and its test selector. Their descriptions depend on
/// the platform (see [`material_note`]).
pub(crate) const MATERIALS: [(MaterialPreference, &str, &str); 2] = [
    (
        MaterialPreference::Glass,
        "Glass",
        "appearance-material-Glass",
    ),
    (
        MaterialPreference::Solid,
        "Solid",
        "appearance-material-Solid",
    ),
];

/// What the page is, in one line: its sidebar entry's description in
/// the search, and its heading's subtitle.
pub(crate) const ABOUT: &str = "Theme and material choices, with a live preview";

/// The Appearance page, registered in the window's page list: the one
/// page of this milestone's Settings whose choices change both windows as
/// they are made.
pub(crate) fn page() -> Page {
    Page {
        title: "Appearance",
        about: ABOUT,
        icon: Glyph::Theme,
        count: None,
        render,
        search: entries,
        focus,
    }
}

/// The page's state, held by the window as a field: each segment's focus,
/// a tab stop, so the keyboard reaches every choice.
pub(crate) struct State {
    focuses: HashMap<&'static str, FocusHandle>,
}

impl State {
    /// The page's state, over the window's `cx` (its focus handles).
    pub(crate) fn new(cx: &mut Context<SettingsWindow>) -> State {
        let selectors = THEMES
            .iter()
            .map(|&(_, _, _, selector)| selector)
            .chain(MATERIALS.iter().map(|&(_, _, selector)| selector));
        let focuses = selectors
            .map(|selector| (selector, cx.focus_handle().tab_stop(true)))
            .collect();
        State { focuses }
    }

    /// The focus of the segment `selector`.
    fn focus(&self, selector: &str) -> FocusHandle {
        self.focuses
            .get(selector)
            .expect("every segment has a focus")
            .clone()
    }
}

/// The settings the page offers the sidebar's search: each choice of the
/// theme and material groups, named as the page names it, in the group
/// it sits in. An override in force leaves the choices listed — the page
/// still shows them — but says why none can be used here, as the page's
/// own notice does.
fn entries(_launcher: &Launcher, cx: &App) -> Vec<search::Entry> {
    let overrides = crate::settings::shared(cx).read(cx).override_descriptions();
    let unavailable = (!overrides.is_empty()).then(|| override_text(&overrides));
    THEMES
        .iter()
        .map(|&(_, name, _, selector)| (name, selector, "Theme"))
        .chain(
            MATERIALS
                .iter()
                .map(|&(_, name, selector)| (name, selector, "Material")),
        )
        .map(|(name, selector, group)| search::Entry {
            control: Some(selector.into()),
            title: name.into(),
            group: Some(group.into()),
            unavailable: unavailable.clone(),
        })
        .collect()
}

/// What an override in force means here, as the page's notice and the
/// search's results say it: which environment variables override what
/// ("`A` overrides", "`A` and `B` override"), and that nothing chosen here
/// applies or is saved.
pub(crate) fn override_text(overrides: &[String]) -> String {
    let verb = if overrides.len() == 1 {
        "overrides"
    } else {
        "override"
    };
    format!(
        "{} {verb} the saved choice for this process: choosing here changes nothing, and Pane \
         does not save it.",
        overrides.join(" and "),
    )
}

/// A jump from the sidebar's search reveals the choice where it drew and
/// leaves the keyboard with the sidebar, as page navigation does: `false`.
/// (Tab reaches the segments from there.)
fn focus(_: &mut SettingsWindow, _: &str, _: &mut Window, _: &mut Context<SettingsWindow>) -> bool {
    false
}

/// Draws the Appearance page: the heading block, the theme and material
/// field groups with their descriptions, whatever the host settings
/// report — an override in force, or a save that failed — and the live
/// preview beside them.
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
    // A choice is offered — focusable, clickable, and saved — only where
    // nothing overrides it for this process; an override is in force
    // otherwise, and the notice above the fields says so.
    let offered = overrides.is_empty();

    let mut theme_segments = Vec::new();
    for (preference, name, _, selector) in THEMES {
        let anchor = this.search_anchor(selector);
        let focus = this.appearance.focus(selector);
        theme_segments.push(segment(
            Segment {
                selector,
                name,
                chosen: preference == theme_preference,
                offered,
            },
            anchor,
            &focus,
            theme,
            move |cx| {
                crate::settings::shared(cx)
                    .update(cx, |settings, cx| settings.set_theme(preference, cx));
            },
        ));
    }
    let mut material_segments = Vec::new();
    for (preference, name, selector) in MATERIALS {
        let anchor = this.search_anchor(selector);
        let focus = this.appearance.focus(selector);
        material_segments.push(segment(
            Segment {
                selector,
                name,
                chosen: preference == material_preference,
                offered,
            },
            anchor,
            &focus,
            theme,
            move |cx| {
                crate::settings::shared(cx)
                    .update(cx, |settings, cx| settings.set_material(preference, cx));
            },
        ));
    }
    let fields = fields(
        (theme_segments, material_segments),
        (theme_preference, material_preference),
        offered,
        theme,
    );
    let notice_in_force = (!offered).then(|| override_notice(&overrides, theme));
    let status = status.map(|status| notice("appearance-status", status, theme.danger, theme));

    // The preview: the miniature launcher with the theme, the material and
    // the bindings in effect.
    let keyboard = crate::settings::keyboard_of(cx);
    let content = preview_content(
        crate::keyboard::binding_keys(keyboard.binding(KeyboardAction::InvokeSelectedAction)),
        &crate::keyboard::binding_keys(keyboard.binding(KeyboardAction::OpenActions)).name(),
    );
    let stage = preview_stage(&content, visuals.material, theme_preference, theme);
    compose(notice_in_force, fields, status, stage, theme).into_any_element()
}

/// The page's composition, which the visual workbench's fixture draws too:
/// the controls column — the heading block, the override notice if one is
/// in force, the field groups and a save's failure, if any, 18px apart —
/// beside the preview column's caption over `stage`, in the page's two
/// columns (see `settings_shell::page_columns`: side by side on the
/// canonical page, the preview below the controls in a narrower window).
pub(crate) fn compose(
    notice_in_force: Option<Stateful<Div>>,
    fields: Vec<Div>,
    status: Option<Stateful<Div>>,
    stage: Stateful<Div>,
    theme: &Theme,
) -> Stateful<Div> {
    let column = controls::column(theme)
        .child(
            settings_shell::page_header("Appearance", Some(ABOUT.into()), theme)
                .id("appearance-title")
                .debug_selector(|| "appearance-title".into()),
        )
        .children(notice_in_force)
        .children(fields)
        .children(status);
    let aside = settings_shell::aside("Preview", theme).child(stage);
    div()
        .id("appearance")
        .debug_selector(|| "appearance".into())
        .child(settings_shell::page_columns(column, aside, theme))
}

/// The page's field groups, which the visual workbench's fixture draws
/// too: the theme's (`segments.0` over the description of `chosen.0`) and
/// the material's (`segments.1` over that of `chosen.1`), offered or not.
pub(crate) fn fields(
    segments: (Vec<Stateful<Div>>, Vec<Stateful<Div>>),
    chosen: (ThemePreference, MaterialPreference),
    offered: bool,
    theme: &Theme,
) -> Vec<Div> {
    let theme_description = notice(
        "appearance-theme-note",
        theme_note(chosen.0),
        theme.text_muted,
        theme,
    );
    let (text, color) = material_note(chosen.1, theme);
    let material_description = notice("appearance-material-note", text, color, theme);
    vec![
        field(
            "theme",
            "Theme",
            segments.0,
            theme_description,
            offered,
            theme,
        ),
        field(
            "material",
            "Material",
            segments.1,
            material_description,
            offered,
            theme,
        ),
    ]
}

/// The theme's description while `preference` is chosen.
pub(crate) fn theme_note(preference: ThemePreference) -> &'static str {
    THEMES
        .iter()
        .find(|&&(choice, ..)| choice == preference)
        .map_or("", |&(_, _, note, _)| note)
}

/// The notice that an override is in force: which environment variables
/// override what, and that nothing chosen here applies or is saved. The
/// fields below show the overridden choices, disabled.
pub(crate) fn override_notice(overrides: &[String], theme: &Theme) -> Stateful<Div> {
    notice(
        "appearance-override",
        override_text(overrides),
        theme.warning,
        theme,
    )
}

/// The preview's stage over the miniature showing `content` on `material`
/// — named for assistive technology by the palette (`theme_preference`)
/// and the surface it shows.
pub(crate) fn preview_stage(
    content: &PreviewContent,
    material: Material,
    theme_preference: ThemePreference,
    theme: &Theme,
) -> Stateful<Div> {
    let miniature = preview::miniature(content, material, theme)
        .debug_selector(|| "appearance-preview-panel".into());
    preview::stage(miniature, theme)
        .id("appearance-preview")
        .debug_selector(|| "appearance-preview".into())
        .role(Role::Image)
        .aria_label(preview_label(theme_preference, material.is_glass()))
}

/// What the preview shows: the board's sample search over Pane's own
/// kinds of results — the query, four pinned tiles, the rows with the
/// first selected — and the footer the launcher draws, with the effective
/// Open actions binding (`open_actions`, by name) in its tip and the
/// effective invoke binding (`invoke`) on its action. Sample content: it
/// is a picture of the launcher, not the user's results.
pub(crate) fn preview_content(invoke: KeySequence, open_actions: &str) -> PreviewContent {
    let row = |title: &str, kind: &str, tone, glyph, selected| PreviewRow {
        title: SharedString::from(title.to_owned()),
        kind: SharedString::from(kind.to_owned()),
        tone,
        glyph,
        selected,
    };
    PreviewContent {
        query: "fig".into(),
        pins: vec![
            (IconTone::Term, Glyph::Prompt),
            (IconTone::Code, Glyph::Code),
            (IconTone::Web, Glyph::Globe),
            (IconTone::Music, Glyph::Music),
        ],
        rows: vec![
            row("Figma", "Application", IconTone::Pen, Glyph::Pen, true),
            row(
                "Settings",
                "Command",
                IconTone::Command,
                Glyph::Sliders,
                false,
            ),
            row(
                "figma-tokens.json",
                "File",
                IconTone::Command,
                Glyph::File,
                false,
            ),
            row(
                "Clipboard History",
                "Command",
                IconTone::Command,
                Glyph::Clipboard,
                false,
            ),
        ],
        tip: Some(format!("{open_actions} for more actions").into()),
        action: "Open".into(),
        keys: invoke,
    }
}

/// The preview's accessible name: what it shows, in words.
fn preview_label(theme: ThemePreference, glass: bool) -> String {
    let palette = match theme {
        ThemePreference::System => "the system's palette",
        ThemePreference::Light => "the light palette",
        ThemePreference::Dark => "the dark palette",
    };
    let surface = if glass {
        "the glass tint"
    } else {
        "the solid surface"
    };
    format!("Preview of the launcher in {palette}, on {surface}")
}

/// One segment's identity and state, as the page resolved it.
struct Segment {
    selector: &'static str,
    name: &'static str,
    /// Whether its choice is the one in effect.
    chosen: bool,
    /// Whether choosing it does anything: nothing is offered while an
    /// override is in force.
    offered: bool,
}

/// One segment of a field's choice ([`controls::segment`]) with its
/// identity and a radio button's semantics. Offered, it is a tab stop
/// that Enter, Space and a click choose — `choose` reports the choice to
/// the host settings, which repaint both windows and save — under Pane's
/// focus ring while the keyboard is on it; not offered, it is neither.
/// `anchor` is the scroll anchor the search's reveal scrolls to.
fn segment(
    segment: Segment,
    anchor: ScrollAnchor,
    focus: &FocusHandle,
    theme: &Theme,
    choose: impl Fn(&mut App) + Clone + 'static,
) -> Stateful<Div> {
    let Segment {
        selector,
        name,
        chosen,
        offered,
    } = segment;
    let drawn = controls::segment(name, chosen, offered, theme)
        .id(selector)
        .debug_selector(move || selector.into())
        .anchor_scroll(Some(anchor))
        .role(Role::RadioButton)
        .aria_label(name)
        .aria_toggled(if chosen {
            Toggled::True
        } else {
            Toggled::False
        });
    if !offered {
        return drawn.aria_disabled(true).cursor_default();
    }
    let ring = controls::segment_focus_shadows(chosen, theme);
    let on_click = choose.clone();
    drawn
        .key_context(CHOICE)
        .track_focus(focus)
        .focus_visible(move |style| style.shadow(ring))
        .on_action(move |_: &Choose, _, cx| choose(cx))
        .on_click(move |_: &gpui::ClickEvent, _, cx| on_click(cx))
}

/// One field group: its label, its segmented choice (`segments` on the
/// track, a radio group named for the label) and its description — the
/// label and the track at the board's disabled opacity while not
/// `offered`, the description, which says why or what holds, at full
/// strength. `id` names the group's debug selectors
/// (`appearance-<id>-field`, `appearance-<id>-track`).
fn field(
    id: &'static str,
    label: &'static str,
    segments: Vec<Stateful<Div>>,
    description: Stateful<Div>,
    offered: bool,
    theme: &Theme,
) -> Div {
    let track = controls::segment_track(theme)
        .id(SharedString::from(format!("appearance-{id}-choices")))
        .debug_selector(move || format!("appearance-{id}-track"))
        .role(Role::RadioGroup)
        .aria_label(label)
        .children(segments);
    let opacity = if offered {
        1.
    } else {
        theme.geometry.controls.disabled_opacity
    };
    controls::field(theme)
        .debug_selector(move || format!("appearance-{id}-field"))
        .child(controls::field_label(label, theme).opacity(opacity))
        .child(track.opacity(opacity))
        .child(description)
}

/// A line the page reports — a field's description, the override notice,
/// a save's failure — in `color`, announced as a status.
fn notice(
    id: &'static str,
    text: impl Into<SharedString>,
    color: gpui::Hsla,
    theme: &Theme,
) -> Stateful<Div> {
    let text = text.into();
    controls::field_description(text.clone(), color, theme)
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Status)
        .aria_label(text)
}

/// The material's description, and its color: the platform's reason (in
/// the warning's color) where glass is chosen but normalizes to the solid
/// surface; what glass is, with the standing caveat that a request is not
/// proof of blur, where a glass request stands; what the solid surface is
/// otherwise.
pub(crate) fn material_note(preference: MaterialPreference, theme: &Theme) -> (String, gpui::Hsla) {
    if preference == MaterialPreference::Solid {
        return (
            "No transparency: an opaque window with a solid panel, the same everywhere.".into(),
            theme.text_muted,
        );
    }
    match crate::ui::material::glass_fallback_reason() {
        Some(reason) => (
            format!(
                "Glass is unavailable here — {reason}. Pane renders the solid surface instead, \
                 and saves the glass choice for a system that provides it."
            ),
            theme.warning,
        ),
        None => (
            "Translucent glass that picks up your wallpaper through the system's blur. Pane \
             cannot see whether the blur was applied; the tint stands on its own if it was not."
                .into(),
            theme.text_muted,
        ),
    }
}
