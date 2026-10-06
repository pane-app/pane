//! The Appearance section of the General page: the theme and material
//! choices (#98), as two rows of segmented choices, and the launcher's
//! background image (ADR 0028) — a row to choose, change or remove the
//! picture, and its effect as a third row of segments.
//!
//! Every value it shows and every choice it takes goes through the host
//! settings ([`crate::settings`]) — the one entity the launcher window and
//! the Settings window both observe — so a choice made here repaints both
//! windows at once, without a restart, and is written to the record off
//! the window's thread. What the section itself owns is presentation only:
//! the rows and the honesty notes (glass's platform fallback, an override
//! in force, a save that failed).
//!
//! It offers only the settings Pane has (#100): the theme (System, Light,
//! Dark), the material (Glass and Solid) and the background image with
//! its effect (None, Dither, ASCII, Halftone, Scanlines — Roboco's
//! new-thread background treatments). The picture is chosen with the
//! platform's file picker; the host settings keep their own copy of it.
//! The development overrides do not touch the background image, and its
//! effect waits for a picture. The board's accent swatches,
//! blur and tint sliders, density, toggles and preview are not Pane's, and
//! nothing draws them.
//!
//! The material's segments carry the *preference*; where the platform
//! cannot provide glass, the material row says so, and the windows render
//! the solid surface.
//!
//! The segments are the keyboard's too: each is a tab stop, and Enter or
//! Space chooses it as a click does, under Pane's focus ring (the board
//! draws none). While an override is in force they are drawn at the
//! board's disabled opacity, and neither take the keyboard nor answer a
//! click.

use std::collections::HashMap;

use gpui::{
    AnyElement, App, Context, Div, FocusHandle, Hsla, KeyBinding, PathPromptOptions, Role,
    ScrollAnchor, SharedString, Stateful, Toggled, actions, div, prelude::*,
};
use pane_core::{BackgroundEffect, Launcher, MaterialPreference, ThemePreference};

use super::{SettingsWindow, search};
use crate::ui::controls;
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

/// The section's label on the General page.
pub(crate) const LABEL: &str = "Appearance";

/// The theme choices the section offers, in segment order: the
/// preference, the segment's name and its test selector.
pub(crate) const THEMES: [(ThemePreference, &str, &str); 3] = [
    (ThemePreference::System, "System", "appearance-theme-System"),
    (ThemePreference::Light, "Light", "appearance-theme-Light"),
    (ThemePreference::Dark, "Dark", "appearance-theme-Dark"),
];

/// The background image's effects the section offers, in segment order:
/// the effect, the segment's name and its test selector.
pub(crate) const EFFECTS: [(BackgroundEffect, &str, &str); 5] = [
    (BackgroundEffect::None, "None", "appearance-effect-None"),
    (
        BackgroundEffect::Dither,
        "Dither",
        "appearance-effect-Dither",
    ),
    (BackgroundEffect::Ascii, "ASCII", "appearance-effect-ASCII"),
    (
        BackgroundEffect::Halftone,
        "Halftone",
        "appearance-effect-Halftone",
    ),
    (
        BackgroundEffect::Scanlines,
        "Scanlines",
        "appearance-effect-Scanlines",
    ),
];

/// The background image row's buttons: Choose (or Change) and Remove.
const CHOOSE_BACKGROUND: &str = "appearance-background-choose";
const REMOVE_BACKGROUND: &str = "appearance-background-remove";

/// The material choices the section offers, in segment order: the
/// preference, the segment's name, and its test selector.
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

/// The section's state, held by the window as a field: each segment's
/// focus, a tab stop, so the keyboard reaches every choice.
pub(crate) struct State {
    focuses: HashMap<&'static str, FocusHandle>,
}

impl State {
    /// The section's state, over the window's `cx` (its focus handles).
    pub(crate) fn new(cx: &mut Context<SettingsWindow>) -> State {
        let selectors = THEMES
            .iter()
            .map(|&(_, _, selector)| selector)
            .chain(MATERIALS.iter().map(|&(_, _, selector)| selector))
            .chain(EFFECTS.iter().map(|&(_, _, selector)| selector))
            .chain([CHOOSE_BACKGROUND, REMOVE_BACKGROUND]);
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

/// The settings the section offers the sidebar's search: each choice of
/// the theme and material rows, named as the section names it, in the row
/// it sits in. An override in force leaves the choices listed (the page
/// still shows them) but says why none can be used here.
pub(crate) fn entries(_launcher: &Launcher, cx: &App) -> Vec<search::Entry> {
    let settings = crate::settings::shared(cx);
    let settings = settings.read(cx);
    let overrides = settings.override_descriptions();
    let unavailable = (!overrides.is_empty()).then(|| override_text(&overrides));
    let mut entries: Vec<search::Entry> = THEMES
        .iter()
        .map(|&(_, name, selector)| (name, selector, "Theme"))
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
        .collect();
    // The background image and its effects, which no override touches;
    // the effects wait for a picture.
    entries.push(search::Entry {
        control: Some(CHOOSE_BACKGROUND.into()),
        title: "Background image".into(),
        group: Some(LABEL.into()),
        unavailable: None,
    });
    let no_picture = settings
        .background()
        .is_none()
        .then(|| NO_PICTURE.to_owned());
    entries.extend(EFFECTS.iter().map(|&(_, name, selector)| search::Entry {
        control: Some(selector.into()),
        title: name.into(),
        group: Some("Background effect".into()),
        unavailable: no_picture.clone(),
    }));
    entries
}

/// Why the effects cannot be chosen yet.
const NO_PICTURE: &str = "Choose a background image first.";

/// What an override in force means here, as the section's notice and the
/// search's results say it: which environment variables set the choice,
/// and that a choice made here neither applies nor is saved.
pub(crate) fn override_text(overrides: &[String]) -> String {
    format!(
        "Set by {} for this session. Changes here won't apply or be saved.",
        overrides.join(" and "),
    )
}

/// What the section shows, as plain values: what [`section`] reads from
/// the host settings.
pub(crate) struct AppearanceView {
    /// The choices in effect.
    pub(crate) theme: ThemePreference,
    pub(crate) material: MaterialPreference,
    /// The environment variables overriding the choices, if any.
    pub(crate) overrides: Vec<String>,
    /// What a save reported, if it failed.
    pub(crate) status: Option<String>,
}

/// The Appearance section as the General page draws it: the theme and
/// material rows with their handlers, from the host settings as they
/// stand.
pub(crate) fn section(this: &mut SettingsWindow, cx: &mut Context<SettingsWindow>) -> Div {
    let settings = crate::settings::shared(cx);
    let (view, background) = {
        let state = settings.read(cx);
        let view = AppearanceView {
            theme: state.theme_preference(),
            material: state.material_preference(),
            overrides: state.override_descriptions(),
            status: state.status(),
        };
        let background = BackgroundView {
            chosen: state.background().is_some(),
            effect: state.background_effect(),
            importing: state.importing_background(),
            problem: state.background_problem(),
        };
        (view, background)
    };
    let theme = crate::settings::visuals(cx).theme;
    // A choice is offered (focusable, clickable and saved) only where
    // nothing overrides it for this process.
    let offered = view.overrides.is_empty();
    let theme_segments = THEMES
        .iter()
        .map(|&(preference, name, selector)| {
            let anchor = this.search_anchor(selector);
            let focus = this.appearance.focus(selector);
            segment(
                Segment {
                    selector,
                    name,
                    chosen: preference == view.theme,
                    offered,
                },
                anchor,
                &focus,
                &theme,
                move |cx| {
                    crate::settings::shared(cx)
                        .update(cx, |settings, cx| settings.set_theme(preference, cx));
                },
            )
        })
        .collect();
    let material_segments = MATERIALS
        .iter()
        .map(|&(preference, name, selector)| {
            let anchor = this.search_anchor(selector);
            let focus = this.appearance.focus(selector);
            segment(
                Segment {
                    selector,
                    name,
                    chosen: preference == view.material,
                    offered,
                },
                anchor,
                &focus,
                &theme,
                move |cx| {
                    crate::settings::shared(cx)
                        .update(cx, |settings, cx| settings.set_material(preference, cx));
                },
            )
        })
        .collect();
    let rows = background_rows(this, &background, &theme);
    compose_with(&view, (theme_segments, material_segments), rows, &theme)
}

/// The background image as the section shows it (ADR 0028).
struct BackgroundView {
    /// Whether a picture is chosen.
    chosen: bool,
    /// The effect chosen for it.
    effect: BackgroundEffect,
    /// Whether a picture is still being copied in.
    importing: bool,
    /// Why the picture is not drawn as chosen, if it is not.
    problem: Option<String>,
}

/// The background image's two rows: the picture — what is chosen, the
/// problem if any, and its Choose (or Change) and Remove buttons — then
/// its effect, as five segments on a wide track, offered once a picture
/// is chosen. Nothing overrides them, so they are offered whatever the
/// theme and material rows say.
fn background_rows(
    this: &mut SettingsWindow,
    background: &BackgroundView,
    theme: &Theme,
) -> Vec<AnyElement> {
    let ready = !background.importing;
    let mut lines = vec![controls::row_line(
        if background.importing {
            "Copying the picture…"
        } else if background.chosen {
            "Drawn behind the launcher, from Pane's own copy of the picture."
        } else {
            "None. Choose a picture to draw behind the launcher."
        },
        theme.text_muted,
        theme,
    )];
    lines.extend(
        background
            .problem
            .as_ref()
            .map(|problem| {
                notice(
                    "appearance-background-problem",
                    problem.clone(),
                    theme.danger,
                    theme,
                )
            })
            .map(IntoElement::into_any_element),
    );
    let anchor = this.search_anchor(CHOOSE_BACKGROUND);
    let choose = button(
        controls::button(
            CHOOSE_BACKGROUND,
            if background.chosen {
                "Change…"
            } else {
                "Choose…"
            },
            ready,
            theme,
        ),
        CHOOSE_BACKGROUND,
        "Choose a background image",
        ready.then(|| this.appearance.focus(CHOOSE_BACKGROUND)),
        theme,
        choose_picture,
    )
    .anchor_scroll(Some(anchor));
    let remove = background.chosen.then(|| {
        button(
            controls::ghost_button(REMOVE_BACKGROUND, "Remove", ready, theme),
            REMOVE_BACKGROUND,
            "Remove the background image",
            ready.then(|| this.appearance.focus(REMOVE_BACKGROUND)),
            theme,
            |cx| {
                crate::settings::shared(cx)
                    .update(cx, |settings, cx| settings.clear_background(cx));
            },
        )
    });
    let picture = controls::setting_row("Background image", lines, theme)
        .debug_selector(|| "appearance-background-field".into())
        .child(
            div()
                .flex_none()
                .flex()
                .gap(theme.geometry.controls.button_gap)
                .children(remove)
                .child(choose),
        );
    let offered = background.chosen;
    let segments = EFFECTS
        .iter()
        .map(|&(effect, name, selector)| {
            let anchor = this.search_anchor(selector);
            let focus = this.appearance.focus(selector);
            segment(
                Segment {
                    selector,
                    name,
                    chosen: effect == background.effect,
                    offered,
                },
                anchor,
                &focus,
                theme,
                move |cx| {
                    crate::settings::shared(cx).update(cx, |settings, cx| {
                        settings.set_background_effect(effect, cx)
                    });
                },
            )
        })
        .collect();
    let track = controls::segment_track(theme)
        .flex_none()
        .w(theme.geometry.settings.wide_choice_width);
    let effect = row_on(
        track,
        "effect",
        "Effect",
        segments,
        Vec::new(),
        offered,
        theme,
    );
    vec![picture.into_any_element(), effect.into_any_element()]
}

/// One of the background row's buttons, `drawn` with the id `selector`
/// and described by `label` for assistive technology. Offered (with a
/// `focus`), it is a tab stop that Enter, Space and a click press —
/// `press` does the button's work — under Pane's focus ring; while a
/// picture is being copied in it is neither.
fn button(
    drawn: Stateful<Div>,
    selector: &'static str,
    label: &'static str,
    focus: Option<FocusHandle>,
    theme: &Theme,
    press: impl Fn(&mut App) + Clone + 'static,
) -> Stateful<Div> {
    let drawn = drawn
        .debug_selector(move || selector.into())
        .role(Role::Button)
        .aria_label(label);
    let Some(focus) = focus else {
        return drawn.aria_disabled(true);
    };
    let ring = controls::focus_ring(theme);
    let on_click = press.clone();
    drawn
        .key_context(CHOICE)
        .track_focus(&focus)
        .focus_visible(move |style| style.shadow(ring))
        .on_action(move |_: &Choose, _, cx| press(cx))
        .on_click(move |_: &gpui::ClickEvent, _, cx| on_click(cx))
}

/// Asks for a picture with the platform's file picker and chooses it as
/// the launcher's background image (the host settings copy it in and
/// record it; see [`crate::settings::Settings::choose_background`]).
/// Cancelling changes nothing; a picker that cannot open is reported.
fn choose_picture(cx: &mut App) {
    let chosen = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Choose".into()),
    });
    cx.spawn(async move |cx| {
        let picked = match chosen.await {
            Ok(Ok(Some(paths))) => paths.into_iter().next().map(Ok),
            Ok(Ok(None)) | Err(_) => None,
            Ok(Err(error)) => Some(Err(format!("Pane could not open a file picker: {error:#}"))),
        };
        if let Some(picked) = picked {
            cx.update(|cx| {
                crate::settings::shared(cx).update(cx, |settings, cx| match picked {
                    Ok(source) => settings.choose_background(source, cx),
                    Err(problem) => settings.background_refused(problem, cx),
                })
            });
        }
    })
    .detach();
}

/// The section's composition: the theme's and the material's rows, with
/// their segments at their ends, and `more` rows in the card after the
/// material's (the background image's, as the General page draws them),
/// then the override notice and a save's failure, if any. While an
/// override is in force the rows' labels and choices are drawn at the
/// disabled opacity.
fn compose_with(
    view: &AppearanceView,
    segments: (Vec<Stateful<Div>>, Vec<Stateful<Div>>),
    more: Vec<AnyElement>,
    theme: &Theme,
) -> Div {
    let offered = view.overrides.is_empty();
    let material_lines = material_note(view.material, theme)
        .map(|(text, color)| notice("appearance-material-note", text, color, theme))
        .into_iter()
        .map(IntoElement::into_any_element)
        .collect();
    let rows = [
        row("theme", "Theme", segments.0, Vec::new(), offered, theme),
        row(
            "material",
            "Material",
            segments.1,
            material_lines,
            offered,
            theme,
        ),
    ];
    let notes: Vec<AnyElement> = (!offered)
        .then(|| {
            notice(
                "appearance-override",
                override_text(&view.overrides),
                theme.warning,
                theme,
            )
        })
        .into_iter()
        .chain(
            view.status
                .as_ref()
                .map(|status| notice("appearance-status", status.clone(), theme.danger, theme)),
        )
        .map(|note| {
            note.px(theme.geometry.settings.section_label_inset)
                .into_any_element()
        })
        .collect();
    let body = div()
        .flex()
        .flex_col()
        .gap(theme.geometry.settings.section_label_gap)
        .child(controls::card(
            rows.into_iter()
                .map(IntoElement::into_any_element)
                .chain(more),
            theme,
        ))
        .children(notes);
    controls::section(Some(LABEL.into()), body, theme).debug_selector(|| "appearance".into())
}

/// One segment's identity and state, as the section resolved it.
struct Segment {
    selector: &'static str,
    name: &'static str,
    /// Whether its choice is the one in effect.
    chosen: bool,
    /// Whether choosing it does anything: nothing is offered while an
    /// override is in force.
    offered: bool,
}

/// One segment of a row's choice ([`controls::segment`]) with its
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

/// One row: its label and `lines` at the left, its segmented choice
/// (`segments` on a row's track, a radio group named for the label) at
/// its end — the label and the track at the board's disabled opacity
/// while not `offered`, the lines, which say why or what holds, at full
/// strength. `id` names the row's debug selectors
/// (`appearance-<id>-field`, `appearance-<id>-track`).
fn row(
    id: &'static str,
    label: &'static str,
    segments: Vec<Stateful<Div>>,
    lines: Vec<AnyElement>,
    offered: bool,
    theme: &Theme,
) -> Div {
    let track = controls::row_segment_track(theme);
    row_on(track, id, label, segments, lines, offered, theme)
}

/// [`row`] with its segments on `track`, a track the caller sized.
fn row_on(
    track: Div,
    id: &'static str,
    label: &'static str,
    segments: Vec<Stateful<Div>>,
    lines: Vec<AnyElement>,
    offered: bool,
    theme: &Theme,
) -> Div {
    let track = track
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
    let label = controls::field_label(label, theme).opacity(opacity);
    controls::setting_row_with(label, lines, theme)
        .debug_selector(move || format!("appearance-{id}-field"))
        .child(track.opacity(opacity))
}

/// A line the section reports (the material's note, the override notice,
/// a save's failure) in `color`, announced as a status.
fn notice(
    id: &'static str,
    text: impl Into<SharedString>,
    color: Hsla,
    theme: &Theme,
) -> Stateful<Div> {
    let text = text.into();
    controls::field_description(text.clone(), color, theme)
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Status)
        .aria_label(text)
}

/// The material row's note, if it needs one, with its color: where glass
/// is chosen but the platform cannot provide it, why, in the warning's
/// color. Glass that stands and Solid need none.
pub(crate) fn material_note(
    preference: MaterialPreference,
    theme: &Theme,
) -> Option<(String, Hsla)> {
    if preference == MaterialPreference::Solid {
        return None;
    }
    crate::ui::material::glass_fallback_reason().map(|reason| {
        (
            format!("Glass isn't available here ({reason}), so Pane uses Solid."),
            theme.warning,
        )
    })
}
