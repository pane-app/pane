//! The workbench's scenarios over Pane's own Settings pages and the
//! launcher's form (#99).
//!
//! Only the Appearance page has an authored board; the other pages are
//! derived compositions of the board's families (`ui::controls`), so these
//! scenarios are native-only: captured, declared and measured against
//! their own declaration, never compared with a board the reference does
//! not have. Each page is drawn through its own production composition
//! (`features::settings::<page>::compose`, the form's parts in
//! `extension_views::form`) over fixture data, inside the Settings board's
//! shell (the board's sidebar, the section nearest the page selected) or
//! the launcher's panel. The data covers what #99 asks native examples of:
//! a select (the Launcher page's, opened by a click), a text form and its
//! validation (the form), shortcut recording (the General page's recorder,
//! listening), extension management and a confirmation (the Extensions
//! page), a long error (the recorder's refusal) and unavailable items (the
//! tray switch, a select choice, a management row).
//!
//! The declaration is the controls' own arithmetic, part by part: a page's
//! column of field groups, each a label over settings rows (a rule along
//! each row's top, its label and description, its control at its right
//! end — a well, a button, a switch), segmented choices, list items and
//! notes, from the theme's tokens and the widths the text system shapes
//! (each label's box rounded up to a whole pixel, as GPUI lays text out).

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Div, Entity, Focusable, FontWeight, Hsla, Pixels, Window, div,
};
use pane_core::hotkeys::Shortcut;
use pane_core::placement::{DisplayLayout, Point};
use pane_core::{KeyboardAction, OpeningMonitor};
use serde::Serialize;

use super::{
    DeclaredCapture, FixtureWindow, Hex, Rect, ResolvedStep, Step, parse_binding, settings_frame,
    shaped_width, wrapped_lines,
};
use crate::extension_views::form;
use crate::features::settings::{about, extensions, general, keyboard, launcher};
use crate::settings;
use crate::ui::footer;
use crate::ui::icon::{Glyph, IconTone};
use crate::ui::keycap::CapStyle;
use crate::ui::material::Material;
use crate::ui::select::{Model, Select};
use crate::ui::shell;
use crate::ui::theme::Theme;

/// Which of Pane's own pages a scenario draws, through the page's own
/// composition (#99).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanePage {
    /// The General page — its recorder listening, with a refusal, or not.
    General { recording: bool },
    /// The Launcher page: its select and its reopening choice.
    Launcher,
    /// The Keyboard page: the actions' recorders, one rebound.
    Keyboard,
    /// The Extensions page: the management list, or a confirmation.
    Extensions { confirm: bool },
    /// The About page.
    About,
    /// The launcher's form screen, after a rejected submission.
    Form,
}

/// The page the scenario `name` draws, if it draws one of Pane's own.
pub(crate) fn pane_page(name: &str) -> Option<PanePage> {
    match name {
        "settings-general" => Some(PanePage::General { recording: false }),
        "settings-general-recording" => Some(PanePage::General { recording: true }),
        "settings-launcher" => Some(PanePage::Launcher),
        "settings-keyboard" => Some(PanePage::Keyboard),
        "settings-extensions" => Some(PanePage::Extensions { confirm: false }),
        "settings-extensions-confirm" => Some(PanePage::Extensions { confirm: true }),
        "settings-about" => Some(PanePage::About),
        "form-validation" => Some(PanePage::Form),
        _ => None,
    }
}

/// The board's sidebar section the page's scenario selects: the one
/// nearest it (the board has no Launcher, Keyboard or Extensions section;
/// its General, Hotkeys & Aliases, Plugins and About stand nearest), or
/// `fallback` for a page with none.
pub(crate) fn selected_section(page: Option<PanePage>, fallback: usize) -> usize {
    match page {
        Some(PanePage::General { .. } | PanePage::Launcher) => 0,
        Some(PanePage::Keyboard) => 2,
        Some(PanePage::Extensions { .. }) => 3,
        Some(PanePage::About) => 7,
        Some(PanePage::Form) | None => fallback,
    }
}

/// The Launcher page's select's trigger, as a click step names it.
pub(crate) const SELECT_TRIGGER: &str = "select-trigger";

/// GPUI's default line height, as a multiple of the text's size (phi): the
/// launcher's screen heading sets none of its own.
const GPUI_LINE: f32 = 1.618_034;

// ------------------------------------------------------------ fixture data

/// The General page's recorder's refusal, while it listens: a long error.
const GENERAL_REFUSAL: &str = "Ctrl+Alt+B cannot be used: another application or the system \
                               already uses it. Choose another combination, or close the \
                               application that holds it and try again.";

/// The tray's state where the switch is not offered: an unavailable item.
const TRAY_STATUS: &str = "Pane's tray icon could not be shown: the notification area did not \
                           answer. The launcher's own menu and the Settings shortcut still \
                           open Settings.";

/// The Keyboard page's rebound action and its binding, so its Reset shows.
const KEYBOARD_REBOUND: (KeyboardAction, &str) = (KeyboardAction::Back, "alt-left");

/// The General page's fixture state.
fn general_view(recording: bool) -> general::GeneralView {
    let shortcut = Shortcut::open_pane_default();
    general::GeneralView {
        keys: crate::keyboard::hotkey_keys(&shortcut),
        binding: shortcut.to_string(),
        default: shortcut.to_string(),
        recording,
        resettable: false,
        problem: None,
        rejection: recording.then(|| GENERAL_REFUSAL.to_owned()),
        login: true,
        login_offered: true,
        login_note: None,
        tray: false,
        tray_offered: false,
        tray_status: Some(TRAY_STATUS.to_owned()),
        tray_refusal: None,
        status: None,
    }
}

/// The Launcher page's fixture state: the defaults, every choice offered.
fn launcher_view() -> launcher::LauncherView {
    launcher::LauncherView {
        unavailable: None,
        fallback: None,
        window_mode: pane_core::WindowMode::Expanded,
        compact_pinned: false,
        pinned_layout: pane_core::PinnedLayout::Horizontal,
        status: None,
    }
}

/// The display layout the fixture's select lists its choices over: the
/// pointer's place known, the active window's not — so the active
/// window's display is listed with its reason, not offered.
fn fixture_layout() -> DisplayLayout {
    DisplayLayout {
        pointer: Some(Point { x: 0., y: 0. }),
        ..DisplayLayout::default()
    }
}

/// The Launcher page's select, as the page makes its own: the production
/// control over the fixture's layout, the primary display committed. It
/// commits nothing anywhere.
pub(crate) fn monitor_select(
    window: &mut Window,
    cx: &mut Context<FixtureWindow>,
) -> Entity<Select> {
    cx.new(|cx| {
        Select::new(
            launcher::MONITOR_NAME,
            launcher::MONITOR_DESCRIPTION,
            launcher::MONITOR_DEBUG,
            Rc::new(|cx: &App| {
                let visuals = settings::visuals(cx);
                Model {
                    theme: visuals.theme,
                    material: visuals.material,
                    choices: launcher::monitor_choices(&fixture_layout()),
                    committed: Some(launcher::monitor_name(OpeningMonitor::Primary).into()),
                }
            }),
            Rc::new(|_: &str, _: &mut Window, _: &mut App| {}),
            window,
            cx,
        )
    })
}

/// The Keyboard page's fixture state: the bindings in force, with one
/// action rebound away from its default.
fn keyboard_view(cx: &App) -> keyboard::KeyboardView {
    let mut rows = keyboard::rows(&settings::keyboard_of(cx));
    let (action, binding) = KEYBOARD_REBOUND;
    if let Some(row) = rows.iter_mut().find(|row| row.action == action) {
        let rebound = parse_binding(binding);
        row.default = Some(row.binding.clone());
        row.keys = crate::keyboard::binding_keys(&rebound);
        row.binding = rebound.to_string();
    }
    keyboard::KeyboardView {
        rows,
        recording: None,
        rejection: None,
        status: None,
        escape: pane_core::EscapeBehavior::default(),
        escape_closes: true,
    }
}

/// One entry of the Extensions page's fixture lists.
fn item(title: &str, reason: Option<&str>) -> extensions::ExtensionItem {
    extensions::ExtensionItem {
        id: title.to_lowercase().replace(' ', "-"),
        title: title.to_owned(),
        reason: reason.map(str::to_owned),
        icon: Some((IconTone::Command, Glyph::Blocks)),
    }
}

/// The Extensions page's fixture state: an installed package's card — one
/// operation unavailable here — and the install sources; or the
/// confirmation disabling a package other packages require asks for.
fn extensions_view(confirm: bool) -> extensions::ExtensionsView {
    if confirm {
        return extensions::ExtensionsView {
            title: "Disable Clipboard History?".into(),
            listing: false,
            status: None,
            details: vec![
                "Notes requires Clipboard History, so disabling it disables Notes too.".into(),
                "Their settings and saved data are kept.".into(),
            ],
            empty: false,
            back: false,
            packages: Vec::new(),
            rows: vec![item("Disable all 2", None), item("Cancel", None)],
            auto_update: None,
            installs: Vec::new(),
        };
    }
    extensions::ExtensionsView {
        title: "Extensions".into(),
        listing: true,
        status: None,
        details: Vec::new(),
        empty: false,
        back: false,
        packages: vec![extensions::PackageCard {
            id: "settings-sample".into(),
            title: "Settings sample".into(),
            icon: Some((IconTone::Command, Glyph::Blocks)),
            enabled: true,
            badges: Vec::new(),
            auto_update: None,
            actions: vec![
                (
                    "reload:settings-sample".into(),
                    "Reload".into(),
                    "Reload Settings sample".into(),
                    None,
                ),
                (
                    "develop:settings-sample".into(),
                    "Develop".into(),
                    "Develop Settings sample".into(),
                    Some("Development needs a build command in the package's manifest".into()),
                ),
                (
                    "uninstall:settings-sample".into(),
                    "Uninstall".into(),
                    "Uninstall Settings sample".into(),
                    None,
                ),
            ],
        }],
        rows: Vec::new(),
        auto_update: Some(("updates".into(), true)),
        installs: extensions::install_items(),
    }
}

/// The About page's fixture state: no check run yet, the documentation
/// opened, the diagnostics copied.
fn about_view(theme: &Theme) -> about::AboutView {
    about::AboutView {
        version: crate::APP_VERSION.to_owned(),
        update_status: ABOUT_UPDATE.into(),
        update_tone: theme.text_muted,
        action: about::Action::Check,
        opened: Some((ABOUT_OPENED.into(), theme.success)),
        copied: true,
    }
}

/// What the About page's fixture says of the update and the opening.
const ABOUT_UPDATE: &str = "Pane has not checked for an update yet.";
const ABOUT_OPENED: &str = "Opened https://github.com/hoangvu12/pane";

/// The form scenario's form: the sample's greeting form, submitted with
/// its name empty.
const FORM_TITLE: &str = "Greet someone";
const FORM_NAME: (&str, &str, &str) = ("name", "Name", "Ada Lovelace");
const FORM_ERROR: &str = "Enter a name";
const FORM_STATUS: &str = "Name: Enter a name";
const FORM_GREETING: (&str, &str) = ("greeting", "Greeting");
const FORM_CHOICES: [(&str, &str); 3] = [
    ("hello", "Hello"),
    ("morning", "Good morning"),
    ("welcome", "Welcome"),
];
const FORM_CHOSEN: usize = 1;
const FORM_SUBMIT: &str = "Greet";

// ------------------------------------------------------------- rendering

/// Pane's own Settings page `page`, through the page's own composition.
pub(crate) fn render_page(
    page: PanePage,
    select: Option<&Entity<Select>>,
    theme: &Theme,
    cx: &App,
) -> AnyElement {
    match page {
        PanePage::General { recording } => general::compose(
            &general_view(recording),
            recording,
            None,
            theme,
            |_, part| part,
        )
        .into_any_element(),
        PanePage::Launcher => {
            let select =
                select.map(|select| div().id("launcher-monitor").w_full().child(select.clone()));
            launcher::compose(&launcher_view(), (select, None), theme, |_, part| part)
                .into_any_element()
        }
        PanePage::Keyboard => {
            keyboard::compose(&keyboard_view(cx), None, theme, |_, part| part).into_any_element()
        }
        PanePage::Extensions { confirm } => {
            extensions::compose(&extensions_view(confirm), theme, |_, part| part).into_any_element()
        }
        PanePage::About => {
            about::compose(&about_view(theme), theme, |_, part| part).into_any_element()
        }
        // The form is a launcher screen, drawn by [`render_form`].
        PanePage::Form => unreachable!("the form scenario is the form family's"),
    }
}

/// The heading and subtitle Settings page `page` shows over its column,
/// as its composition draws them; `None` for the form, which is a launcher
/// screen.
pub(crate) fn heading(page: PanePage) -> Option<(String, Option<String>)> {
    let fixed = |title: &str, subtitle: &str| Some((title.to_owned(), Some(subtitle.to_owned())));
    match page {
        PanePage::General { .. } => fixed(general::TITLE, general::ABOUT),
        PanePage::Launcher => fixed(launcher::TITLE, launcher::ABOUT),
        PanePage::Keyboard => fixed(keyboard::TITLE, keyboard::ABOUT),
        PanePage::Extensions { confirm } => Some((extensions_view(confirm).title, None)),
        PanePage::About => fixed(about::TITLE, about::ABOUT),
        PanePage::Form => None,
    }
}

/// The launcher's form screen after a rejected submission, through the
/// form's own parts and the launcher's screen heading: the name field
/// (the fixture's own editable field, focused) with its error under it,
/// the greeting's segmented choice, the submit button, and the footer's
/// status naming the rejection.
pub(crate) fn render_form(fixture: &FixtureWindow, theme: &Theme, cx: &App) -> Div {
    let (name_id, name_label, placeholder) = FORM_NAME;
    let name = form::text_control(
        form::TextControl {
            index: 0,
            id: name_id,
            label: name_label,
            value: "",
            placeholder,
            error: Some(FORM_ERROR),
        },
        &fixture.query,
        &fixture.query.focus_handle(cx),
        theme,
    );
    let (greeting_id, greeting_label) = FORM_GREETING;
    let segments = FORM_CHOICES
        .iter()
        .enumerate()
        .map(|(position, &(choice, label))| {
            form::choice_segment(
                (greeting_id, choice, label),
                (position, FORM_CHOICES.len()),
                position == FORM_CHOSEN,
                theme,
            )
        })
        .collect();
    let greeting = form::choice_track((1, greeting_id, greeting_label), None, segments, theme);
    let fields = vec![
        form::field_group(
            name_id,
            name_label.to_owned(),
            name,
            Some(FORM_ERROR.to_owned()),
            theme,
        )
        .into_any_element(),
        form::field_group(
            greeting_id,
            greeting_label.to_owned(),
            greeting,
            None,
            theme,
        )
        .into_any_element(),
    ];
    let body = form::compose(
        FORM_TITLE.to_owned(),
        fields,
        form::submit_button(FORM_SUBMIT.to_owned(), theme),
        theme,
    );
    // The strip carries the status's tone, as the launcher's does.
    let footer = Material::footer(theme)
        .id("status")
        .text_size(theme.typography.footer_size)
        .text_color(theme.danger)
        .child(footer::footer_row(
            footer::mark_button(theme).into_any_element(),
            footer::status_message(FORM_STATUS, theme).into_any_element(),
            footer::buttons(None, None, theme),
            theme,
        ));
    div()
        .size_full()
        .flex()
        .flex_col()
        .font_family(theme.typography.family.clone())
        .font_features(theme.typography.features.clone())
        .text_color(theme.text_title)
        .child(shell::screen_heading(FORM_TITLE, theme))
        .child(body)
        .child(footer)
}

// ------------------------------------------------------------ declaration

/// One part of a page as a capture declares it: what kind of part it is
/// (a `rule` along a settings row's top; a `label` or `text` line; a
/// field's `well`; a `button`; a `toggle`; a segmented choice's `track`
/// and its `segment`s; a list `item`), its name, its box, its color where
/// it has one (a line's ink), whether it is on (a toggle, a chosen
/// segment) and the opacity it is drawn at.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredPart {
    kind: &'static str,
    name: String,
    rect: Rect,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<Hex>,
    #[serde(skip_serializing_if = "Option::is_none")]
    on: Option<bool>,
    opacity: f32,
}

/// A page as a capture declares it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredPage {
    capture: &'static str,
    page: String,
    /// Whether the select's list is open in this capture: the popover and
    /// its shadow then lie over the page (and reach the sidebar beside it).
    select_open: bool,
    parts: Vec<DeclaredPart>,
    colors: PageColors,
}

/// The fills the page's parts paint, as the theme in force holds them.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PageColors {
    field_fill: Hex,
    field_edge: Hex,
    button_fill: Hex,
    segment_track: Hex,
    segment_on: Hex,
    toggle_off: Hex,
    accent: Hex,
    hairline_soft: Hex,
}

/// What sits at a settings row's right end, right to left as declared
/// left to right: a well `width` wide, a ghost button with its label (and
/// whether it is offered), a switch (on, offered).
enum Trailing<'a> {
    Well {
        name: &'a str,
        width: f32,
    },
    Ghost {
        label: &'a str,
        enabled: bool,
    },
    Toggle {
        name: &'a str,
        on: bool,
        enabled: bool,
    },
}

/// A cursor down a page's column, declaring its parts as `ui::controls`
/// stacks them.
struct Column<'a> {
    window: &'a Window,
    theme: &'a Theme,
    x: f32,
    width: f32,
    y: f32,
    parts: Vec<DeclaredPart>,
}

impl Column<'_> {
    /// A text box's width as layout gives it: the shaped width, rounded up
    /// to a whole pixel.
    fn shaped(&self, text: &str, size: Pixels, weight: FontWeight) -> f32 {
        shaped_width(self.window, self.theme, text, size, weight).ceil()
    }

    fn push(&mut self, kind: &'static str, name: &str, rect: Rect) -> &mut DeclaredPart {
        self.parts.push(DeclaredPart {
            kind,
            name: name.to_owned(),
            rect,
            color: None,
            on: None,
            opacity: 1.,
        });
        self.parts.last_mut().expect("just pushed")
    }

    /// A line of text at (x, y): its first line's box, in `color` at
    /// `opacity`; returns how many lines it wraps to in `width`.
    fn line(
        &mut self,
        (kind, text): (&'static str, &str),
        (x, y, width): (f32, f32, f32),
        (size, weight, line): (Pixels, FontWeight, Pixels),
        (color, opacity): (Hsla, f32),
    ) -> usize {
        let lines = wrapped_lines(self.window, self.theme, text, (size, weight), width);
        let shaped = self.shaped(text, size, weight).min(width);
        let part = self.push(
            kind,
            text,
            Rect {
                x,
                y,
                width: shaped,
                height: f32::from(line),
            },
        );
        part.color = Some(Hex(color));
        part.opacity = opacity;
        lines
    }

    /// The page's heading block — the heading, its subtitle's lines, the
    /// block's margin — then the column's gap. The shell declares the
    /// heading's own ink.
    fn heading(&mut self, subtitle: Option<&str>) {
        let settings = &self.theme.geometry.settings;
        let lines = &self.theme.typography.settings;
        let f = f32::from;
        self.y += f(lines.heading.line_height);
        if let Some(subtitle) = subtitle {
            let style = (lines.subtitle.size, self.theme.typography.regular);
            let count = wrapped_lines(self.window, self.theme, subtitle, style, self.width);
            self.y += f(settings.header_gap) + count as f32 * f(lines.subtitle.line_height);
        }
        self.y += f(settings.header_margin_bottom) + f(self.theme.geometry.controls.group_gap);
    }

    /// A field's label (13.5/500), then the field's gap.
    fn label(&mut self, text: &str) {
        let theme = self.theme;
        let line = theme.typography.settings.field_label;
        let (x, y, width) = (self.x, self.y, self.width);
        self.line(
            ("label", text),
            (x, y, width),
            (line.size, theme.typography.medium, line.line_height),
            (theme.text_title, 1.),
        );
        self.y += f32::from(line.line_height) + f32::from(theme.geometry.controls.field_gap);
    }

    /// A description under what came before (the field's gap above it), in
    /// `color`.
    fn note(&mut self, text: &str, color: Hsla) {
        let theme = self.theme;
        let line = theme.typography.settings.field_description;
        self.y += f32::from(theme.geometry.controls.field_gap);
        let (x, y, width) = (self.x, self.y, self.width);
        let count = self.line(
            ("text", text),
            (x, y, width),
            (line.size, theme.typography.regular, line.line_height),
            (color, 1.),
        );
        self.y += count as f32 * f32::from(line.line_height);
    }

    /// The gap between field groups.
    fn next_group(&mut self) {
        self.y += f32::from(self.theme.geometry.controls.group_gap);
    }

    /// A settings row: its rule, its label over its `lines` (each in its
    /// color), and `trailing` at its right end, the whole row at
    /// `opacity`.
    fn row(
        &mut self,
        label: &str,
        lines: &[(&str, Hsla)],
        trailing: &[Trailing<'_>],
        opacity: f32,
    ) {
        let theme = self.theme;
        let controls = &theme.geometry.controls;
        let typography = &theme.typography;
        let f = f32::from;
        let label_line = typography.settings.field_label;
        let description = typography.settings.field_description;
        // The controls, right to left from the row's end.
        let mut right = self.x + self.width;
        let mut placed = Vec::new();
        for (index, part) in trailing.iter().rev().enumerate() {
            if index > 0 {
                right -= f(controls.button_gap);
            }
            let width = match part {
                Trailing::Well { width, .. } => *width,
                Trailing::Ghost { label, .. } => {
                    2. * f(theme.geometry.results.pill_padding_x)
                        + self.shaped(label, typography.results.pill.size, typography.medium)
                }
                Trailing::Toggle { .. } => f(controls.toggle_width),
            };
            right -= width;
            placed.push((right, width, part));
        }
        let text_width = right - f(controls.row_gap) - self.x;
        let wraps: Vec<usize> = lines
            .iter()
            .map(|(text, _)| {
                let style = (description.size, typography.regular);
                wrapped_lines(self.window, theme, text, style, text_width)
            })
            .collect();
        let text_height = f(label_line.line_height)
            + wraps.iter().sum::<usize>() as f32 * f(description.line_height);
        let padded = !lines.is_empty();
        let height = if padded {
            (1. + 2. * f(controls.row_padding_y) + text_height).max(f(controls.toggle_row_height))
        } else {
            f(controls.toggle_row_height)
        };
        let top = self.y;
        // Centered in the box below the 1px rule (inside the padding).
        let center = top + (height + 1.) / 2.;
        let rule = self.push(
            "rule",
            label,
            Rect {
                x: self.x,
                y: top,
                width: self.width,
                height: 1.,
            },
        );
        rule.opacity = opacity;
        let mut text_top = center - text_height / 2.;
        let x = self.x;
        self.line(
            ("label", label),
            (x, text_top, text_width),
            (label_line.size, typography.regular, label_line.line_height),
            (theme.text_title, opacity),
        );
        text_top += f(label_line.line_height);
        for ((text, color), count) in lines.iter().zip(&wraps) {
            self.line(
                ("text", *text),
                (x, text_top, text_width),
                (
                    description.size,
                    typography.regular,
                    description.line_height,
                ),
                (*color, opacity),
            );
            text_top += *count as f32 * f(description.line_height);
        }
        for (left, width, part) in placed {
            match part {
                Trailing::Well { name, .. } => {
                    let well = f(controls.inline_well_height);
                    self.push(
                        "well",
                        name,
                        Rect {
                            x: left,
                            y: center - well / 2.,
                            width,
                            height: well,
                        },
                    )
                    .opacity = opacity;
                }
                Trailing::Ghost { label, enabled } => {
                    let line = typography.results.pill;
                    let label_x = left + f(theme.geometry.results.pill_padding_x);
                    let label_y = center - f(line.line_height) / 2.;
                    let label_width = self.shaped(label, line.size, typography.medium);
                    let dim = if *enabled {
                        1.
                    } else {
                        controls.disabled_opacity
                    };
                    self.line(
                        ("text", *label),
                        (label_x, label_y, label_width),
                        (line.size, typography.medium, line.line_height),
                        (theme.text_body, opacity * dim),
                    );
                }
                Trailing::Toggle { name, on, enabled } => {
                    let toggle_height = f(controls.toggle_height);
                    let part = self.push(
                        "toggle",
                        name,
                        Rect {
                            x: left,
                            y: center - toggle_height / 2.,
                            width,
                            height: toggle_height,
                        },
                    );
                    part.on = Some(*on);
                    part.opacity = if *enabled {
                        opacity
                    } else {
                        controls.disabled_opacity
                    };
                }
            }
        }
        self.y = top + height;
    }

    /// A field's well on its own line (34 high, the column's width), named
    /// `name`.
    fn well(&mut self, name: &str) -> Rect {
        let height = f32::from(self.theme.geometry.controls.well_height);
        let rect = Rect {
            x: self.x,
            y: self.y,
            width: self.width,
            height,
        };
        self.push("well", name, rect);
        self.y += height;
        rect
    }

    /// A segmented choice of `labels`, the one at `chosen` chosen, named
    /// `name`.
    fn track(&mut self, name: &str, labels: &[&str], chosen: usize) {
        let theme = self.theme;
        let controls = &theme.geometry.controls;
        let f = f32::from;
        let rect = Rect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: f(controls.segment_height) + 2. * f(controls.track_padding),
        };
        self.push("track", name, rect);
        for (index, (segment, label)) in super::segment_rects(theme, rect, labels.len())
            .into_iter()
            .zip(labels)
            .enumerate()
        {
            self.push("segment", label, segment).on = Some(index == chosen);
        }
        self.y += rect.height;
    }

    /// A button (`.pill`) at the column's start, saying `label`.
    fn button(&mut self, name: &str, label: &str) {
        let theme = self.theme;
        let pill = &theme.geometry.results;
        let typography = &theme.typography;
        let width = 2. * f32::from(pill.pill_padding_x)
            + self.shaped(label, typography.results.pill.size, typography.medium);
        let height = f32::from(pill.pill_height);
        let rect = Rect {
            x: self.x,
            y: self.y,
            width,
            height,
        };
        self.push("button", name, rect);
        self.y += height;
    }

    /// The field's gap, between a field's parts.
    fn gap(&mut self) {
        self.y += f32::from(self.theme.geometry.controls.field_gap);
    }

    /// A list item: `label` over its lines (each one line), its tile; the
    /// items of a list 2px apart.
    fn item(&mut self, label: &str, lines: usize) {
        let theme = self.theme;
        let controls = &theme.geometry.controls;
        let settings = &theme.geometry.settings;
        let typography = &theme.typography;
        let f = f32::from;
        let text_height = f(typography.settings.field_label.line_height)
            + lines as f32 * f(typography.settings.field_description.line_height);
        let height = if lines > 0 {
            (2. * f(controls.row_padding_y) + text_height).max(f(controls.toggle_row_height))
        } else {
            f(controls.toggle_row_height)
        };
        let rect = Rect {
            x: self.x,
            y: self.y,
            width: self.width,
            height,
        };
        self.push("item", label, rect);
        let tile = f(theme.geometry.tile.size);
        let text_x = self.x + f(settings.item_padding_x) + tile + f(controls.row_gap);
        let text_top = self.y + (height - text_height) / 2.;
        let line = typography.settings.field_label;
        let width = self.width - (text_x - self.x);
        self.line(
            ("label", label),
            (text_x, text_top, width),
            (line.size, typography.medium, line.line_height),
            (theme.text_title, 1.),
        );
        self.y += height + f(controls.list_gap);
    }
}

/// The well holding `keys`' caps (a recorder's): at least its minimum
/// width, the caps inside its padding.
fn recorder_width(window: &Window, theme: &Theme, keys: &crate::ui::keycap::KeySequence) -> f32 {
    let controls = &theme.geometry.controls;
    let caps = super::keys_width(window, theme, keys, CapStyle::Regular);
    (2. * f32::from(controls.well_padding_x) + caps).max(f32::from(controls.recorder_min_width))
}

/// The listening mark's well.
fn listening_width(window: &Window, theme: &Theme) -> f32 {
    let controls = &theme.geometry.controls;
    let line = theme.typography.settings.segment;
    let mark = shaped_width(
        window,
        theme,
        "Press the keys…",
        line.size,
        theme.typography.medium,
    )
    .ceil();
    (2. * f32::from(controls.well_padding_x) + mark).max(f32::from(controls.recorder_min_width))
}

/// `page` as its scenario's captures declare it (it holds still: no step
/// changes what it shows, except the select's click, whose popup is the
/// capture's evidence), with each click step resolved to the center of the
/// part it names.
pub(crate) fn declare(
    window: &Window,
    theme: &Theme,
    (client, page): ((f32, f32), PanePage),
    steps: &mut [ResolvedStep],
    captures: &mut [DeclaredCapture],
    cx: &App,
) -> Vec<DeclaredPage> {
    let parts = declared_parts(window, theme, client, page, cx);
    let trigger = parts
        .iter()
        .find(|part| part.name == SELECT_TRIGGER)
        .map(|part| part.rect.center());
    let mut pointer = None;
    let mut opened = false;
    let mut declared = Vec::new();
    for step in steps.iter_mut() {
        match step.step {
            Step::Click { target } if target == SELECT_TRIGGER => {
                step.point = trigger;
                pointer = trigger;
                opened = true;
            }
            Step::Capture { name } => {
                if let Some(capture) = captures.iter_mut().find(|capture| capture.name == name) {
                    capture.pointer = pointer;
                }
                // The open select's list covers the page below its trigger:
                // only the trigger is declared then, the list's crop the
                // evidence.
                let mut parts = declared_parts(window, theme, client, page, cx);
                if opened {
                    parts.retain(|part| part.name == SELECT_TRIGGER);
                }
                declared.push(DeclaredPage {
                    capture: name,
                    page: format!("{page:?}"),
                    select_open: opened,
                    parts,
                    colors: PageColors {
                        field_fill: Hex(theme.field_fill),
                        field_edge: Hex(theme.field_edge),
                        button_fill: Hex(theme.results.pill_fill),
                        segment_track: Hex(theme.controls.segment_track),
                        segment_on: Hex(theme.controls.segment_on),
                        toggle_off: Hex(theme.controls.toggle_off),
                        accent: Hex(theme.accent),
                        hairline_soft: Hex(theme.hairline_soft),
                    },
                });
            }
            _ => {}
        }
    }
    declared
}

/// The parts `page` lays out in a client of `client` size.
fn declared_parts(
    window: &Window,
    theme: &Theme,
    client: (f32, f32),
    page: PanePage,
    cx: &App,
) -> Vec<DeclaredPart> {
    let f = f32::from;
    let mut column = if page == PanePage::Form {
        // The launcher's screen: the heading (GPUI's default line, phi, at
        // 14px, between its 12px paddings), then the form's own 12px.
        let geometry = &theme.geometry;
        let heading =
            2. * f(geometry.screen_padding_y) + f(theme.typography.row_title_size) * GPUI_LINE;
        Column {
            window,
            theme,
            x: f(geometry.search_padding_x),
            width: client.0 - 2. * f(geometry.search_padding_x),
            y: heading + f(geometry.screen_padding_y),
            parts: Vec::new(),
        }
    } else {
        let frame = settings_frame(theme, client);
        let settings = &theme.geometry.settings;
        Column {
            window,
            theme,
            x: frame.heading.x,
            width: frame.page.width - 2. * f(settings.page_padding_x),
            y: frame.heading.y,
            parts: Vec::new(),
        }
    };
    let muted = theme.text_muted;
    match page {
        PanePage::General { recording } => {
            let view = general_view(recording);
            column.heading(Some(general::ABOUT));
            column.label("Open Pane");
            let hint = if recording {
                crate::ui::controls::RECORDING_TEXT
            } else {
                general::RECORDER_HINT
            };
            let width = if recording {
                listening_width(window, theme)
            } else {
                recorder_width(window, theme, &view.keys)
            };
            column.row(
                "Open Pane hotkey",
                &[(hint, muted)],
                &[
                    Trailing::Ghost {
                        label: "Reset",
                        enabled: view.resettable,
                    },
                    Trailing::Well {
                        name: "open-pane-recorder",
                        width,
                    },
                ],
                1.,
            );
            if let Some(rejection) = &view.rejection {
                column.note(rejection, theme.danger);
            }
            column.next_group();
            column.label("Startup");
            column.row(
                "Launch Pane at login",
                &[("Pane is ready when you log in", muted)],
                &[Trailing::Toggle {
                    name: "launch-at-login",
                    on: view.login,
                    enabled: view.login_offered,
                }],
                1.,
            );
            column.next_group();
            column.label(general::tray_group_title());
            let dimmed = theme.geometry.controls.disabled_opacity;
            column.row(
                general::tray_row_title(),
                &[],
                &[Trailing::Toggle {
                    name: "tray-visibility",
                    on: view.tray,
                    enabled: view.tray_offered,
                }],
                if view.tray_offered { 1. } else { dimmed },
            );
            if let Some(status) = &view.tray_status {
                column.note(status, theme.warning);
            }
        }
        PanePage::Launcher => {
            column.heading(Some(launcher::ABOUT));
            column.label(launcher::MONITOR_NAME);
            column.well(SELECT_TRIGGER);
            column.note(launcher::MONITOR_DESCRIPTION, muted);
            column.next_group();
            column.label(launcher::REOPENING_NAME);
            column.well(launcher::REOPENING_DEBUG);
        }
        PanePage::Keyboard => {
            let view = keyboard_view(cx);
            column.heading(Some(keyboard::ABOUT));
            column.label("In-app navigation");
            for row in &view.rows {
                let lines: Vec<(&str, Hsla)> = Vec::new();
                let well = Trailing::Well {
                    name: row.action.id(),
                    width: recorder_width(window, theme, &row.keys),
                };
                let trailing: Vec<Trailing<'_>> = match row.default {
                    Some(_) => vec![
                        Trailing::Ghost {
                            label: "Reset",
                            enabled: true,
                        },
                        well,
                    ],
                    None => vec![well],
                };
                column.row(row.action.title(), &lines, &trailing, 1.);
            }
        }
        PanePage::Extensions { confirm } => {
            let view = extensions_view(confirm);
            column.heading(None);
            if !view.details.is_empty() {
                let line = theme.typography.settings.field_description;
                for detail in &view.details {
                    let (x, y, width) = (column.x, column.y, column.width);
                    let count = column.line(
                        ("text", detail),
                        (x, y, width),
                        (line.size, theme.typography.regular, line.line_height),
                        (theme.text_body, 1.),
                    );
                    column.y +=
                        count as f32 * f(line.line_height) + f(theme.geometry.controls.list_gap);
                }
                // The block's last gap is not the block's.
                column.y -= f(theme.geometry.controls.list_gap);
                column.next_group();
            }
            for card in &view.packages {
                column.item(&card.title, 0);
            }
            for row in &view.rows {
                column.item(&row.title, usize::from(row.reason.is_some()));
            }
        }
        PanePage::About => {
            column.heading(Some(about::ABOUT));
            let view = about_view(theme);
            column.label(&format!("Pane {}", view.version));
            column.next_group();
            column.label("Updates");
            column.note_at_top(ABOUT_UPDATE, view.update_tone);
            column.gap();
            column.button("about-check-update", about::CHECK_LABEL);
            column.next_group();
            column.label("Documentation");
            column.note_at_top(about::DOCUMENTATION_NOTE, muted);
            column.gap();
            column.button("about-documentation", about::DOCUMENTATION_LABEL);
            column.note(ABOUT_OPENED, theme.success);
            column.next_group();
            column.label("Diagnostics");
            column.note_at_top(about::DIAGNOSTICS_NOTE, muted);
            column.gap();
            column.button("about-diagnostics", about::DIAGNOSTICS_LABEL);
            column.note(about::COPIED, theme.success);
        }
        PanePage::Form => {
            let (name_id, name_label, _) = FORM_NAME;
            column.label(name_label);
            column.well(&format!("field-{name_id}"));
            column.note(FORM_ERROR, theme.danger);
            column.next_group();
            let (greeting_id, greeting_label) = FORM_GREETING;
            column.label(greeting_label);
            let labels: Vec<&str> = FORM_CHOICES.iter().map(|&(_, label)| label).collect();
            column.track(&format!("field-{greeting_id}"), &labels, FORM_CHOSEN);
            column.next_group();
            column.button("submit", FORM_SUBMIT);
        }
    }
    column.parts
}

impl Column<'_> {
    /// A description right under a field's label (whose gap is already
    /// taken), in `color`.
    fn note_at_top(&mut self, text: &str, color: Hsla) {
        let theme = self.theme;
        let line = theme.typography.settings.field_description;
        let (x, y, width) = (self.x, self.y, self.width);
        let count = self.line(
            ("text", text),
            (x, y, width),
            (line.size, theme.typography.regular, line.line_height),
            (color, 1.),
        );
        self.y += count as f32 * f32::from(line.line_height);
    }
}
