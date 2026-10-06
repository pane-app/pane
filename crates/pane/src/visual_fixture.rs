//! The Windows reference/native comparison workbench's native fixture (#91).
//!
//! This is the mechanism every visual ticket of the UI port (#90) uses: a
//! window that renders Pane's **production** GPUI components — the result
//! row, the keycap, the search header, the L1 panel and footer surfaces,
//! the footer action button — at the authored reference's client size,
//! with fixture data copied from the reference board, in named states a
//! capture helper drives. It is not a second UI: every visual value comes
//! from the same [`Theme`] tokens the launcher renders with, and the
//! components are the same functions the launcher calls. The composition
//! glue around them (which rows, which state, which keycaps) is this
//! harness's own, because the workbench owns scenario state, not the
//! launcher.
//!
//! The scenarios are the registration seam: a registry of [`Scenario`]s —
//! each a name, the client size it renders at, the rows it shows and the
//! [`Step`]s that drive it — that this ticket starts with the root/result
//! and keycap families. Later tickets register their own scenarios (the
//! contextual Actions panel, the calculator card, the clipboard split
//! view, the Appearance controls) by appending to [`scenarios`];
//! [`pending_scenarios`] records the reference boards whose native
//! scenarios do not exist yet, each with the ticket that will add them, so
//! the workbench reports them as pending rather than silently passing
//! them by (none is left since the Appearance page, #98). Store and the
//! snap HUD stay source-only fixture references: their boards are
//! catalogued in the research, and no production or fixture
//! implementation is planned for them in this milestone.
//!
//! The steps are the one description of a scenario both capture helpers
//! follow — the native one against this window, the reference one against
//! the authored board — and [`replay`] replays them over the
//! fixture's own state model, so the manifest declares, for every capture,
//! which rows show, which is selected, which the pointer is over and where
//! each one lies. The comparison then validates each side against its own
//! declaration before comparing the sides with each other.
//!
//! What the fixture is **not**: it is not a parity test by itself. The
//! comparison that proves 1:1 fidelity lives in the workbench's scripts
//! (`scripts/visual-workbench/`). The fixture also never touches the
//! user's data: the runner gives it its own temporary data directory, it
//! loads no extensions and reads no installed commands, and the fixture
//! rows below are presentation values only — they never become the user's
//! installed-app list. The real launcher's own wiring is guarded
//! separately, by
//! `the_production_scenario_edits_searches_selects_opens_and_back_navigates`
//! in the window integration tests: fixture matching cannot hide a missing
//! production path.

use std::path::{Path, PathBuf};

use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Focusable, FontWeight, Hsla, Pixels, Role,
    ScrollHandle, SharedString, TextRun, TitlebarOptions, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, size,
};
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged};
use pane_core::clipboard::CaptureState;
use pane_core::clipboard_view::{ClipboardFilter, capture_summary};
use pane_core::{
    Binding, ComputedAnswer, KeyboardAction, ResultAction, ResultActionItem, ResultActions,
    SelectedAction,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::app::{KEY_CONTEXT, action_button};
use crate::features::actions_panel;
use crate::features::clipboard_history;
use crate::features::root_search::{self, search_header};
use crate::features::settings::appearance;
use crate::settings;
use crate::ui::controls;
use crate::ui::footer;
use crate::ui::icon::{Glyph, IconTone, TileSize, tile_at};
use crate::ui::input::bind_text_editing;
use crate::ui::keycap::{self, CapMetrics, CapStyle};
use crate::ui::material::{Material, MaterialMode};
use crate::ui::pinned::{self, SlotContent};
use crate::ui::preview::{self, PreviewContent, PreviewRow};
use crate::ui::result_layouts::{self, AnswerCard, AnswerSide, HistoryRow, NoticeCopy, Suggestion};
use crate::ui::result_row::{RowContent, RowMeta, result_row_with};
use crate::ui::settings_shell::{self, SidebarItem};
use crate::ui::shell::{self, LAUNCHER_CLIENT, SectionLabel};
use crate::ui::split_view::{self, ClipMark, ClipRow, ClipTone};
use crate::ui::theme::{Theme, TypeLine};
use crate::{Back, SelectNext, SelectPrevious};

mod pages;

/// The root reference board's client size, in logical pixels: the authored
/// 760×518 (64 search header + 404 list + 50 footer). Every root-family
/// fixture renders at exactly this client size — the launcher window's own
/// ([`LAUNCHER_CLIENT`]); the comparison never rescales an image to hide a
/// mismatch.
pub(crate) const ROOT_CLIENT: (f32, f32) = LAUNCHER_CLIENT;

/// A narrow launcher client: no reference board is this size. The launcher
/// window can be resized this small, and the narrow scenario shows that
/// the selected row and the footer stay visible and reachable there — an
/// adaptation, captured but never compared.
pub(crate) const NARROW_CLIENT: (f32, f32) = (480., 360.);

/// The Settings reference board's client size: the authored 1120×720 (48
/// titlebar + 672 body) — the Settings window's own
/// ([`settings_shell::SETTINGS_CLIENT`]).
pub(crate) const SETTINGS_CLIENT: (f32, f32) = settings_shell::SETTINGS_CLIENT;

/// A smaller Settings client: no reference board is this size. The
/// Settings window can be this small, and the narrow scenario shows the
/// smaller-window policy there (see `ui::settings_shell`) — an adaptation,
/// captured but never compared.
pub(crate) const SETTINGS_NARROW_CLIENT: (f32, f32) = (760., 520.);

/// The clipboard reference board's client size, for the fixture that
/// ticket #102 registers.
pub(crate) const CLIPBOARD_CLIENT: (f32, f32) = (940., 600.);

/// The keycap scenario's inset from the panel edge and the gap between
/// its caps — the harness's own layout, chosen so each cap stands alone
/// on the panel for measuring.
const KEYCAP_INSET: f32 = 20.;
const KEYCAP_GAP: f32 = 12.;

/// One fixture row: presentation values matching the authored reference
/// root board's own fixture data (its `Suggested` and `Commands` sections
/// at an empty query), so the native and reference captures show the
/// same content wherever the production components can represent it.
/// Kind, alias and shortcut metadata the reference also authors have no
/// production presentation yet — mismatches the comparison reports, not
/// ones this harness papers over. The icons are the reference's own tones
/// and glyphs, drawn by the production tile.
#[derive(Debug, PartialEq)]
pub(crate) struct FixtureRow {
    pub(crate) title: &'static str,
    pub(crate) subtitle: Option<&'static str>,
    pub(crate) unavailable: Option<&'static str>,
    pub(crate) icon: (IconTone, Glyph),
    /// The primary action's label the footer shows while the row is
    /// selected, as the reference board names it for the row's kind.
    pub(crate) action: &'static str,
    /// The row's kind, right-aligned ("Application", "Command").
    pub(crate) kind: &'static str,
    /// The alias the reference gives the row, if any.
    pub(crate) alias: Option<&'static str>,
    /// The hotkey the reference shows on the row, as a binding, if any.
    pub(crate) keys: Option<&'static str>,
}

impl FixtureRow {
    /// What the launcher's Actions panel lists for this row (see
    /// `pane_core::Launcher::result_actions`): its primary action, then
    /// pinning it, for a command or an application, then, for a command —
    /// the fixture's commands stand in for installed ones — its hotkey and
    /// alias configuration, named by the core's rule for whether it has
    /// them.
    fn actions(&self) -> ResultActions {
        let mut items = vec![ResultActionItem {
            action: ResultAction::Invoke,
            label: self.action.to_owned(),
            available: self.unavailable.is_none(),
        }];
        // A command or an application is a result a quick slot can hold:
        // the fixture's pins are not its rows, so it can be pinned (#101).
        if self.kind == "Command" || self.kind == "Application" {
            items.push(ResultActionItem {
                action: ResultAction::Pin,
                label: ResultAction::Pin
                    .quick_slot_label()
                    .expect("a quick slot entry")
                    .to_owned(),
                available: true,
            });
        }
        if self.kind == "Command" {
            for (action, configured) in [
                (ResultAction::Hotkey, self.keys.is_some()),
                (ResultAction::Alias, self.alias.is_some()),
            ] {
                items.push(ResultActionItem {
                    action,
                    label: action
                        .configuration_label(configured)
                        .expect("a configuration entry")
                        .to_owned(),
                    available: true,
                });
            }
        }
        ResultActions {
            target: self.title.to_owned(),
            title: self.title.to_owned(),
            items,
        }
    }

    /// This row with the alias `alias`.
    const fn aliased(self, alias: &'static str) -> FixtureRow {
        FixtureRow {
            alias: Some(alias),
            ..self
        }
    }

    /// This row with the hotkey `binding`.
    const fn keyed(self, binding: &'static str) -> FixtureRow {
        FixtureRow {
            keys: Some(binding),
            ..self
        }
    }
}

/// An application row: the reference's `app()`, opened by "Open
/// Application".
const fn app(title: &'static str, icon: (IconTone, Glyph)) -> FixtureRow {
    FixtureRow {
        title,
        subtitle: None,
        unavailable: None,
        icon,
        action: "Open Application",
        kind: "Application",
        alias: None,
        keys: None,
    }
}

/// A command row: the reference's `cmd()`, run by "Run Command".
const fn command(
    title: &'static str,
    subtitle: &'static str,
    icon: (IconTone, Glyph),
) -> FixtureRow {
    FixtureRow {
        title,
        subtitle: Some(subtitle),
        unavailable: None,
        icon,
        action: "Run Command",
        kind: "Command",
        alias: None,
        keys: None,
    }
}

/// The reference root board's rows, in its order, with the kind, alias
/// and hotkey each carries there: its Suggested section's four (Figma,
/// Clipboard History, Left Half, Search Files) then its Commands
/// section's four (Plugin Store, Toggle Dark Mode, Lock Screen,
/// Settings). The fixture labels them as production does — one
/// "Commands" section for a blank query, never a claim of recent use
/// (#100) — so the reference's second label is a content difference the
/// comparison reports. The scenarios over these rows show the board's
/// pinned home above them ([`ROOT_PINS`], #101).
pub(crate) const ROOT_ROWS: &[FixtureRow] = &[
    app("Figma", (IconTone::Pen, Glyph::Pen)),
    command(
        "Clipboard History",
        "Clipboard",
        (IconTone::Command, Glyph::Clipboard),
    )
    .aliased("cb")
    .keyed("ctrl-shift-v"),
    command(
        "Left Half",
        "Window Manager",
        (IconTone::Command, Glyph::Layout),
    )
    .keyed("win-alt-left"),
    command("Search Files", "Files", (IconTone::Command, Glyph::File)).aliased("f"),
    command("Plugin Store", "Pane", (IconTone::Command, Glyph::Blocks)).aliased("store"),
    command(
        "Toggle Dark Mode",
        "System",
        (IconTone::Command, Glyph::Moon),
    ),
    command("Lock Screen", "System", (IconTone::Command, Glyph::Lock)),
    command("Settings", "Pane", (IconTone::Command, Glyph::Sliders)).keyed("ctrl-,"),
];

/// A long-content row: the title and subtitle the production row's
/// truncation policy has to handle. The reference authors no long-content
/// row, so this state has no reference counterpart — it is a native-only
/// crop, recorded honestly as an adaptation rather than compared. The long
/// row carries an alias and keys too: the text truncates, its right-hand
/// parts never shrink.
pub(crate) const LONG_ROWS: &[FixtureRow] = &[
    command(
        "A result whose title runs far past the reference's own fixture rows and keeps going",
        "and whose subtitle is long enough to need the ellipsis the row owns",
        (IconTone::Command, Glyph::Prompt),
    )
    .aliased("long")
    .keyed("ctrl-shift-v"),
    command(
        "Left Half",
        "Window Manager",
        (IconTone::Command, Glyph::Layout),
    ),
];

/// A row that cannot run here, with the reason the production row shows.
/// The reference authors no unavailable state either: another native-only
/// adaptation, cropped and recorded as such. Its alias and keys stay
/// beside a reason long enough to wrap.
pub(crate) const UNAVAILABLE_ROWS: &[FixtureRow] = &[
    command(
        "Left Half",
        "Window Manager",
        (IconTone::Command, Glyph::Layout),
    ),
    FixtureRow {
        unavailable: Some(
            "The extension that provides this command is disabled; turn it back on in Settings to run it here",
        ),
        ..command(
            "Clipboard History",
            "Clipboard",
            (IconTone::Command, Glyph::Clipboard),
        )
        .aliased("cb")
        .keyed("ctrl-shift-v")
    },
];

/// One fixture pin: a quick slot's presentation values, copied from the
/// reference root board's `pinnedRaw` — authored samples for the visual
/// comparison only, never the user's pins (#101: a fresh installation
/// pins nothing).
#[derive(Debug, PartialEq)]
pub(crate) struct FixturePin {
    pub(crate) title: &'static str,
    pub(crate) icon: (IconTone, Glyph),
    /// Why the slot's target cannot run, for the native-only partial home.
    pub(crate) unavailable: Option<&'static str>,
}

/// A pin of `title` with the reference's `tone` and `glyph`.
const fn sample_pin(title: &'static str, tone: IconTone, glyph: Glyph) -> FixturePin {
    FixturePin {
        title,
        icon: (tone, glyph),
        unavailable: None,
    }
}

/// The reference root board's five pins, in its order: Terminal, Visual
/// Studio Code, Firefox, Obsidian and Spotify, with its tones and glyphs.
/// They fill the strip's one row, so no pin hint follows them.
pub(crate) const ROOT_PINS: &[FixturePin] = &[
    sample_pin("Terminal", IconTone::Term, Glyph::Prompt),
    sample_pin("Visual Studio Code", IconTone::Code, Glyph::Code),
    sample_pin("Firefox", IconTone::Web, Glyph::Globe),
    sample_pin("Obsidian", IconTone::Note, Glyph::Notes),
    sample_pin("Spotify", IconTone::Music, Glyph::Music),
];

/// A partly filled home, which the reference never authors: three pins,
/// the last unavailable with its reason, and the pin hint in the cell
/// after them.
pub(crate) const PARTIAL_PINS: &[FixturePin] = &[
    sample_pin("Terminal", IconTone::Term, Glyph::Prompt),
    sample_pin("Firefox", IconTone::Web, Glyph::Globe),
    FixturePin {
        title: "Obsidian",
        icon: (IconTone::Note, Glyph::Notes),
        unavailable: Some("Notes is disabled"),
    },
];

/// The launcher's kind of a fixture row's kind label: an application's
/// primary action opens it (the Actions panel's arrow glyph).
fn row_kind(kind: &str) -> Option<pane_core::RowKind> {
    match kind {
        "Application" => Some(pane_core::RowKind::Application),
        "Command" => Some(pane_core::RowKind::Command),
        "File" => Some(pane_core::RowKind::File),
        "Fallback" => Some(pane_core::RowKind::Fallback),
        _ => None,
    }
}

/// The reference Actions board's rows: its "fig" query's four results
/// and its fallback, in its order, with Figma selected. The file row has
/// no tone of its own among the production tiles, so it shows the command
/// tile with the file glyph — a difference the comparison reports.
pub(crate) const ACTIONS_ROWS: &[FixtureRow] = &[
    app("Figma", (IconTone::Pen, Glyph::Pen)),
    command(
        "Recent Figma Files",
        "Design Files",
        (IconTone::Command, Glyph::File),
    ),
    FixtureRow {
        action: "Open File",
        kind: "File",
        ..command(
            "figma-tokens.json",
            "~/Design/tokens",
            (IconTone::Command, Glyph::File),
        )
    },
    command(
        "Configure Pane",
        "Settings",
        (IconTone::Command, Glyph::Sliders),
    ),
    FixtureRow {
        action: "Search Web",
        kind: "Fallback",
        ..command(
            "Search the web for “fig”",
            "Default browser",
            (IconTone::Command, Glyph::Globe),
        )
    },
];

/// A fallback row: the reference's `Fallback` rows, which send the text
/// typed to what they name, with the board's label for that action.
const fn fallback(
    title: &'static str,
    subtitle: &'static str,
    glyph: Glyph,
    action: &'static str,
) -> FixtureRow {
    FixtureRow {
        action,
        kind: "Fallback",
        ..command(title, subtitle, (IconTone::Command, glyph))
    }
}

/// The row a computed answer is: drawn as the answer card, with no kind
/// and no subtitle, copied by `action`.
const fn answer_row(title: &'static str, action: &'static str) -> FixtureRow {
    FixtureRow {
        subtitle: None,
        action,
        kind: "",
        ..command(title, "", (IconTone::Command, Glyph::Calculator))
    }
}

/// A calculation-history row: `expression` and `answer`, under `kind`.
const fn history(
    expression: &'static str,
    answer: &'static str,
    kind: &'static str,
    glyph: Glyph,
) -> FixtureRow {
    FixtureRow {
        action: "Copy Answer",
        kind,
        ..command(expression, answer, (IconTone::Command, glyph))
    }
}

/// The reference empty board's fallbacks for "kubectx", in its order:
/// fixture content standing in for the fallbacks a user chose (Pane's own
/// rows are titled with the command they send the text to).
pub(crate) const EMPTY_ROWS: &[FixtureRow] = &[
    fallback(
        "Search the web for “kubectx”",
        "Default browser",
        Glyph::Globe,
        "Search Web",
    ),
    fallback(
        "Search files for “kubectx”",
        "Files",
        Glyph::File,
        "Search Files",
    ),
    fallback(
        "Create Script Command “kubectx”",
        "Scripts",
        Glyph::Terminal,
        "Create Command",
    ),
];

/// The reference calculator board's rows: the answer its card shows, then
/// its recent calculations — fixture content: Pane's calculator answers
/// arithmetic, converts no units and keeps no history (#100).
pub(crate) const CALCULATOR_ROWS: &[FixtureRow] = &[
    answer_row("182.88 cm", "Copy Answer"),
    history("1920 / 16 × 9", "= 1,080", "Math", Glyph::Calculator),
    history("15% of 2,400", "= 360", "Math", Glyph::Calculator),
    history(
        "3 pm Stockholm in Tokyo",
        "= 22:00",
        "Time zone",
        Glyph::Clock,
    ),
];

/// A computed answer as production presents it: the calculator's answer
/// to "6*7", copied by the launcher's own "Copy answer".
pub(crate) const ANSWER_ROWS: &[FixtureRow] = &[answer_row("42", "Copy answer")];

/// A computed answer whose expression is too long for the authored size.
pub(crate) const LONG_ANSWER_ROWS: &[FixtureRow] = &[answer_row("123456887764", "Copy answer")];

/// A static result board's composition around its rows (#96): the
/// reference's empty and calculator boards, and the production shapes of
/// the same parts. Every part is drawn by the shared result layouts
/// (`crate::ui::result_layouts`); what a board holds beyond what Pane's
/// launcher holds — units, conversions, history, suggestions — is fixture
/// content, never production data (#100).
#[derive(Debug)]
pub(crate) struct ResultBoard {
    /// The query the board shows: its search field holds it from the
    /// start, and its rows are listed for it as authored, unfiltered.
    pub(crate) query: &'static str,
    /// Whether the no-results notice heads the list.
    pub(crate) notice: bool,
    /// The answer card, which the board's first row is drawn as.
    pub(crate) answer: Option<FixtureAnswer>,
    /// Whether the rows after the card are its calculation history.
    pub(crate) history: bool,
    /// The section labels the board authors over its rows (first row,
    /// label, note), where they are its content, not root search's own.
    pub(crate) sections: Option<&'static [(usize, &'static str, &'static str)]>,
    /// The extension suggestions after the rows, under their label.
    pub(crate) suggestions: Option<FixtureSuggestions>,
}

/// An answer card's values, each with its caption where the board
/// authors one, its "Also" chips, and the command that computed it.
#[derive(Debug)]
pub(crate) struct FixtureAnswer {
    /// The command whose title labels the card in root search's sections.
    pub(crate) command: &'static str,
    pub(crate) source: (&'static str, Option<&'static str>),
    pub(crate) answer: (&'static str, Option<&'static str>),
    pub(crate) also: &'static [&'static str],
}

impl FixtureAnswer {
    /// The computed answer this is, when it holds only what a computed
    /// answer does: no captions and no chips (the production shapes).
    fn computed(&self) -> Option<ComputedAnswer> {
        let plain = self.source.1.is_none() && self.answer.1.is_none() && self.also.is_empty();
        plain.then(|| ComputedAnswer {
            query: self.source.0.to_owned(),
            answer: self.answer.0.to_owned(),
            command: self.command.to_owned(),
        })
    }

    /// The card as drawn: a production-shaped answer through the
    /// launcher's own composition from its computed answer, an authored
    /// one through the shared card with the board's content.
    fn draw(&self, selected: bool, theme: &Theme) -> gpui::Div {
        match self.computed() {
            Some(computed) => root_search::layouts::answer_card(&computed, selected, theme),
            None => result_layouts::answer_card(&self.card(selected), theme),
        }
    }

    /// The card's accessible name: the launcher's own for a computed
    /// answer ("6*7 = 42"), else the row's `title`.
    fn label(&self, title: &str) -> String {
        match self.computed() {
            Some(computed) => root_search::layouts::answer_label(&computed),
            None => title.to_owned(),
        }
    }

    /// The shared card's values for this answer.
    fn card(&self, selected: bool) -> AnswerCard {
        let side = |(value, caption): (&'static str, Option<&'static str>)| AnswerSide {
            value: value.into(),
            caption: caption.map(Into::into),
        };
        AnswerCard {
            source: side(self.source),
            answer: side(self.answer),
            also: self.also.iter().map(|&chip| chip.into()).collect(),
            selected,
        }
    }
}

/// A board's extension suggestions: their label, the label's note and the
/// keys ending it (as a binding), and the suggestions.
#[derive(Debug)]
pub(crate) struct FixtureSuggestions {
    pub(crate) label: &'static str,
    pub(crate) note: &'static str,
    pub(crate) keys: &'static str,
    pub(crate) items: &'static [FixtureSuggestion],
}

/// One extension suggestion, as the board authors it.
#[derive(Debug)]
pub(crate) struct FixtureSuggestion {
    pub(crate) title: &'static str,
    pub(crate) meta: &'static str,
    pub(crate) glyph: Glyph,
    /// The tile's inline colors on the board (`0xRRGGBBAA`): its fill and
    /// its glyph's — per extension, data rather than a theme role.
    pub(crate) tile: (u32, u32),
    pub(crate) action: &'static str,
}

impl FixtureSuggestion {
    /// The shared suggestion row's values.
    fn suggestion(&self) -> Suggestion {
        let color = |hex: u32| gpui::rgb_to_hsla(gpui::rgba(hex));
        Suggestion {
            title: self.title.into(),
            meta: self.meta.into(),
            glyph: self.glyph,
            tile: (color(self.tile.0), color(self.tile.1)),
            action: self.action.into(),
        }
    }
}

/// The empty board: "kubectx" matches nothing; the notice heads the three
/// fallbacks, then two extensions "From the Plugin Store".
static EMPTY_BOARD: ResultBoard = ResultBoard {
    query: "kubectx",
    notice: true,
    answer: None,
    history: false,
    sections: None,
    suggestions: Some(FixtureSuggestions {
        label: "From the Plugin Store",
        note: "See all",
        keys: "ctrl-enter",
        items: &[
            FixtureSuggestion {
                title: "Kube Context",
                meta: "@tamsin · 31 KB · Reads your kubeconfig",
                glyph: Glyph::Package,
                tile: (0x173352FF, 0x8FC3FFFF),
                action: "Install",
            },
            FixtureSuggestion {
                title: "Kubernetes Clusters",
                meta: "@okoro · 58 KB · Runs kubectl locally",
                glyph: Glyph::Target,
                tile: (0x163B3FFF, 0x86D9E0FF),
                action: "Install",
            },
        ],
    }),
};

/// The notice with no fallback under it: what production shows a user who
/// chose none.
static NO_FALLBACKS_BOARD: ResultBoard = ResultBoard {
    query: "kubectx",
    notice: true,
    answer: None,
    history: false,
    sections: None,
    suggestions: None,
};

/// The calculator board: "72 in to cm" answered by the card, with its
/// units, conversions and recent calculations under the board's labels.
static CALCULATOR_BOARD: ResultBoard = ResultBoard {
    query: "72 in to cm",
    notice: false,
    answer: Some(FixtureAnswer {
        command: "Calculator",
        source: ("72 in", Some("Inches")),
        answer: ("182.88 cm", Some("Centimeters")),
        also: &["1.8288 m", "6 ft", "2 yd"],
    }),
    history: true,
    sections: Some(&[
        (0, "Calculator", "Units"),
        (1, "Recent calculations", "Stays on this device"),
    ]),
    suggestions: None,
};

/// A computed answer as production presents it: what was typed and the
/// answer, no captions or chips, under the command's title.
static PLAIN_ANSWER_BOARD: ResultBoard = ResultBoard {
    query: "6*7",
    notice: false,
    answer: Some(FixtureAnswer {
        command: "Calculator",
        source: ("6*7", None),
        answer: ("42", None),
        also: &[],
    }),
    history: false,
    sections: None,
    suggestions: None,
};

/// A computed answer whose expression is too long for the authored 34px:
/// its values step down to fit their columns.
static LONG_ANSWER_BOARD: ResultBoard = ResultBoard {
    query: "123456789 * 1000 + 98765 - 1",
    notice: false,
    answer: Some(FixtureAnswer {
        command: "Calculator",
        source: ("123456789 * 1000 + 98765 - 1", None),
        answer: ("123456887764", None),
        also: &[],
    }),
    history: false,
    sections: None,
    suggestions: None,
};

/// The result board the scenario `name` renders, if it renders one (#96).
fn result_board(name: &str) -> Option<&'static ResultBoard> {
    match name {
        "empty-state" => Some(&EMPTY_BOARD),
        "empty-no-fallbacks" => Some(&NO_FALLBACKS_BOARD),
        "calculator-card" => Some(&CALCULATOR_BOARD),
        "answer-plain" => Some(&PLAIN_ANSWER_BOARD),
        "answer-long" => Some(&LONG_ANSWER_BOARD),
        _ => None,
    }
}

/// Which component family a scenario renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Family {
    /// The root search composition: the search header, the result rows,
    /// the footer with its action button.
    Root,
    /// The keycap family, as the reference's Windows key groups pair
    /// against the production caps.
    Keycap,
    /// The icon tile family at each of its sizes.
    Tiles,
    /// The Settings window's shell: its titlebar, its sidebar with the
    /// search field and the section items, and its page's heading block
    /// and columns (#97).
    Settings,
    /// The split view: Clipboard History's list beside its preview (#102).
    Clipboard,
    /// A launcher screen of the launcher's own: the form, drawn with the
    /// Settings field families (#99).
    Form,
}

/// One step a capture helper takes, on either side. The helpers act with
/// real input — the native one through Windows' own pointer and keyboard,
/// the reference one through the browser's input events — never by
/// setting state behind the window's back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub(crate) enum Step {
    /// Save a capture of the client, named.
    Capture { name: &'static str },
    /// Move the pointer onto the center of the shown row at `row` (in the
    /// Settings family, the sidebar's section at `row`), `nudge` pixels to
    /// the right of it, arriving from a pixel to its left (two moves, as a
    /// real pointer reports): a second move over the same row is movement
    /// only if it lands somewhere else.
    Pointer { row: usize, nudge: i16 },
    /// Press a key.
    Key { key: NamedKey },
    /// Type text into the focused field: the query, or the Actions
    /// panel's search while the panel is open.
    Type { text: &'static str },
    /// Click the element the fixture declares as `target` at its center
    /// (the footer's Actions button), the pointer moving there first.
    Click { target: &'static str },
    /// Move the pointer onto the center of the element the fixture
    /// declares as `target` (a pinned slot, `slot-<n>`), arriving from a
    /// pixel to its left, without pressing.
    Point { target: &'static str },
}

/// The second pinned slot, as a point step names it.
pub(crate) const SLOT_2: &str = "slot-2";

/// The pointer onto `target`.
const fn point(target: &'static str) -> Step {
    Step::Point { target }
}

/// The footer's Actions button, as a click step names it.
pub(crate) const ACTIONS_BUTTON: &str = "actions-button";

/// A click on `target`.
const fn click(target: &'static str) -> Step {
    Step::Click { target }
}

/// A key a step presses: the selection keys and Back under their default
/// bindings, which both capture helpers name the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum NamedKey {
    Down,
    Escape,
}

const fn capture(name: &'static str) -> Step {
    Step::Capture { name }
}

/// The pointer onto the center of row `row`.
const fn pointer(row: usize) -> Step {
    Step::Pointer { row, nudge: 0 }
}

/// The selection key that moves to the next row.
const DOWN: Step = Step::Key {
    key: NamedKey::Down,
};

/// A scenario the workbench renders now, with production components.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Scenario {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) family: Family,
    /// The logical client size the fixture window must have for this
    /// scenario. The runner verifies the measured client against it.
    pub(crate) client: (f32, f32),
    /// Whether the authored reference has this state. A native-only
    /// scenario (an adaptation the reference never authors) is captured
    /// and kept as evidence, never compared and never counted as parity.
    pub(crate) reference: bool,
    /// The reference board the scenario pairs with, when it is not the
    /// interactive root board: a static board authors its one state, so
    /// the reference side captures it as authored and takes no steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) board: Option<&'static str>,
    /// The appearance the scenario renders in, when it is not the run's
    /// own (`--theme`): the light frame is captured in the same run as
    /// the dark ones. Light is a derived palette, so a light scenario is
    /// always native-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) theme: Option<&'static str>,
    /// Whether the comparison measures the launcher's frame in this
    /// scenario's captures — the panel's inset edges, its corners, and
    /// where the header, list and footer begin and end (#92).
    pub(crate) frame: bool,
    #[serde(skip)]
    pub(crate) rows: &'static [FixtureRow],
    /// The pinned home's pins over a blank query, in order; none at all
    /// for a scenario that shows no home.
    #[serde(skip)]
    pub(crate) pins: &'static [FixturePin],
    /// What the capture helpers do, in order, from the scenario's rest.
    pub(crate) steps: &'static [Step],
}

/// The scenarios this ticket starts the registry with: the root/result
/// family and the keycap family, over the production components that
/// exist today. Downstream tickets append theirs here — that is the
/// registration seam. Every scenario starts at rest (an empty, focused
/// query; the first row selected; the pointer outside the window) and
/// captures every state along its way, not only its endpoint.
pub(crate) fn scenarios() -> &'static [Scenario] {
    SCENARIOS
}

const SCENARIOS: &[Scenario] = {
    use Step::{Key, Type};
    &[
        Scenario {
            name: "root-rest",
            description: "The reference's empty query over its own fixture rows, row 0 selected",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: true,
            board: None,
            theme: None,
            frame: false,
            rows: ROOT_ROWS,
            pins: ROOT_PINS,
            steps: &[capture("rest")],
        },
        Scenario {
            name: "root-hover",
            description: "Rest, then the pointer moves onto unselected row 1, which selects it",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: true,
            board: None,
            theme: None,
            frame: false,
            rows: ROOT_ROWS,
            pins: ROOT_PINS,
            steps: &[capture("rest"), pointer(1), capture("hover")],
        },
        Scenario {
            name: "root-selected",
            description: "Rest, then the selection keys move the selection to row 2, one step at a time",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: true,
            board: None,
            theme: None,
            frame: false,
            rows: ROOT_ROWS,
            pins: ROOT_PINS,
            steps: &[
                capture("rest"),
                Key {
                    key: NamedKey::Down,
                },
                capture("down-1"),
                Key {
                    key: NamedKey::Down,
                },
                capture("selected"),
            ],
        },
        Scenario {
            name: "root-pointer-keys",
            description: "The pointer selects row 1; Down moves the selection to row 2 with the pointer resting on row 1, which keeps its hover wash; moving again over row 1 selects it",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: true,
            board: None,
            theme: None,
            frame: false,
            rows: ROOT_ROWS,
            pins: ROOT_PINS,
            steps: &[
                pointer(1),
                capture("pointed"),
                DOWN,
                capture("down-under-pointer"),
                Step::Pointer { row: 1, nudge: 6 },
                capture("moved-again"),
            ],
        },
        Scenario {
            name: "root-selected-hover",
            description: "Row 2 selected by the keys, then the pointer arrives on that selected row",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: true,
            board: None,
            theme: None,
            frame: false,
            rows: ROOT_ROWS,
            pins: ROOT_PINS,
            steps: &[
                Key {
                    key: NamedKey::Down,
                },
                Key {
                    key: NamedKey::Down,
                },
                capture("selected"),
                pointer(2),
                capture("selected-hover"),
            ],
        },
        Scenario {
            name: "root-focus",
            description: "Typing 'clip' into the focused field filters to the one matching row; Escape returns to rest",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: true,
            board: None,
            theme: None,
            frame: false,
            rows: ROOT_ROWS,
            pins: ROOT_PINS,
            steps: &[
                capture("rest"),
                Type { text: "clip" },
                capture("focus-typed"),
                Key {
                    key: NamedKey::Escape,
                },
                capture("back-to-rest"),
            ],
        },
        Scenario {
            name: "root-actions",
            description: "Down selects Clipboard History; the footer's Actions button opens its actions over the dimmed results; typing filters them to one, then to none; Escape closes the panel only",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: true,
            board: None,
            theme: None,
            frame: false,
            rows: ROOT_ROWS,
            pins: ROOT_PINS,
            steps: &[
                DOWN,
                capture("selected"),
                click(ACTIONS_BUTTON),
                capture("open"),
                Type { text: "alias" },
                capture("filtered"),
                Type { text: "zz" },
                capture("empty"),
                Key {
                    key: NamedKey::Escape,
                },
                capture("closed"),
            ],
        },
        Scenario {
            name: "actions-panel",
            description: "The Actions board: the query 'fig' with Figma selected and its actions open over the dimmed results",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: true,
            board: Some("actions"),
            theme: None,
            frame: false,
            rows: ACTIONS_ROWS,
            pins: &[],
            steps: &[Type { text: "fig" }, click(ACTIONS_BUTTON), capture("open")],
        },
        Scenario {
            name: "root-unavailable",
            description: "A row that cannot run here, with its reason (no reference counterpart)",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: UNAVAILABLE_ROWS,
            pins: &[],
            steps: &[
                capture("rest"),
                Key {
                    key: NamedKey::Down,
                },
                capture("unavailable-selected"),
            ],
        },
        Scenario {
            name: "root-long-content",
            description: "A long title and subtitle under the row's truncation (no reference counterpart)",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: LONG_ROWS,
            pins: &[],
            steps: &[capture("long-content")],
        },
        Scenario {
            name: "launcher-frame",
            description: "The launcher's frame at rest: the 760x518 panel, its inset edges and corners, and the header, list and footer boundaries",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: true,
            board: None,
            theme: None,
            frame: true,
            rows: ROOT_ROWS,
            pins: ROOT_PINS,
            steps: &[capture("frame")],
        },
        Scenario {
            name: "launcher-frame-light",
            description: "The same frame in the derived light appearance, at the same dimensions (no reference counterpart)",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: false,
            board: None,
            theme: Some("light"),
            frame: true,
            rows: ROOT_ROWS,
            pins: ROOT_PINS,
            steps: &[capture("frame-light")],
        },
        Scenario {
            name: "launcher-frame-narrow",
            description: "A narrow launcher: the keys select the last row, which the list scrolls into view above the footer (no reference counterpart)",
            family: Family::Root,
            client: NARROW_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: true,
            rows: ROOT_ROWS,
            pins: ROOT_PINS,
            steps: &[
                capture("narrow-rest"),
                DOWN,
                DOWN,
                DOWN,
                DOWN,
                DOWN,
                DOWN,
                DOWN,
                capture("narrow-last-selected"),
            ],
        },
        Scenario {
            name: "tile-sizes",
            description: "The icon tile at the row's, a pinned slot's and the Actions header's size, as an application and as a command (no reference counterpart side by side)",
            family: Family::Tiles,
            client: ROOT_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("tiles")],
        },
        Scenario {
            name: "keycap-windows",
            description: "The production key sequences for the reference's Windows chords, its accent Enter and a compact pinned hint, and rebound chords the reference never shows",
            family: Family::Keycap,
            client: ROOT_CLIENT,
            reference: true,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("keycaps")],
        },
        Scenario {
            name: "settings-shell",
            description: "The Settings board's shell: the titlebar, the sidebar's search and sections with Appearance selected, the page's heading block and columns; then the pointer over an unselected section, and over the selected one",
            family: Family::Settings,
            client: SETTINGS_CLIENT,
            reference: true,
            board: Some("settings"),
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[
                capture("rest"),
                pointer(0),
                capture("hover"),
                pointer(SETTINGS_SELECTED),
                capture("selected-hover"),
            ],
        },
        Scenario {
            name: "settings-shell-narrow",
            description: "The same shell at 760x520: the titlebar and sidebar keep their size and the page's columns collapse into one (no reference counterpart)",
            family: Family::Settings,
            client: SETTINGS_NARROW_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("narrow")],
        },
        Scenario {
            name: "appearance-page",
            description: "The Settings board's Appearance page: Material with Glass chosen and its note, the accent swatches, the blur and tint sliders, Density and the two toggles, beside the preview's stage and miniature launcher; then the pointer over Frost, whose label lightens (#98). The swatches, sliders, Frost, density and toggles are the board's, deferred in production (#100)",
            family: Family::Settings,
            client: SETTINGS_CLIENT,
            reference: true,
            board: Some("settings"),
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[
                capture("rest"),
                point(SEGMENT_FROST),
                capture("segment-hover"),
            ],
        },
        Scenario {
            name: "appearance-solid",
            description: "A click on Solid: its segment takes the wash, its note replaces Glass's, the blur and tint sliders keep their values at the board's 40% disabled opacity, and the miniature takes the solid surface (#98)",
            family: Family::Settings,
            client: SETTINGS_CLIENT,
            reference: true,
            board: Some("settings"),
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[click(SEGMENT_SOLID), capture("solid")],
        },
        Scenario {
            name: "appearance-production",
            description: "Production's own Appearance page through its composition: Theme (System, Light, Dark) and Material (Glass, Solid) with their descriptions, beside the preview in the appearance in effect (no reference counterpart: the board shows no theme choice)",
            family: Family::Settings,
            client: SETTINGS_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("production")],
        },
        Scenario {
            name: "appearance-light",
            description: "Production's Appearance page in the derived light palette: Light chosen, the preview light (no reference counterpart)",
            family: Family::Settings,
            client: SETTINGS_CLIENT,
            reference: false,
            board: None,
            theme: Some("light"),
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("light")],
        },
        Scenario {
            name: "appearance-override",
            description: "Production's Appearance page under the run's development overrides: the notice naming them, both fields' labels and choices at the disabled opacity (no reference counterpart)",
            family: Family::Settings,
            client: SETTINGS_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("override")],
        },
        Scenario {
            name: "appearance-narrow",
            description: "Production's Appearance page at 760x520: the fields take the page's width and the preview wraps below them (no reference counterpart)",
            family: Family::Settings,
            client: SETTINGS_NARROW_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("narrow")],
        },
        Scenario {
            name: "settings-general",
            description: "Pane's General page through its own composition (#99): the Open Pane hotkey's settings row with its recorder's well and the disabled Reset, the launch-at-login switch on, the tray switch not offered with its reason (no reference counterpart: the board has no such page)",
            family: Family::Settings,
            client: SETTINGS_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("general")],
        },
        Scenario {
            name: "settings-general-recording",
            description: "The General page's recorder listening under its focus ring, with a refused combination's long error under the row (no reference counterpart)",
            family: Family::Settings,
            client: SETTINGS_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("recording")],
        },
        Scenario {
            name: "settings-launcher",
            description: "Pane's Launcher page (#99): the opening monitor's select as a field (its trigger a well) and the reopening choice as segments; then a click opens the select's list, the committed choice highlighted and an unanswered one listed with its reason (no reference counterpart)",
            family: Family::Settings,
            client: SETTINGS_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[
                capture("closed"),
                click(pages::SELECT_TRIGGER),
                capture("open"),
            ],
        },
        Scenario {
            name: "settings-keyboard",
            description: "Pane's Keyboard page (#99): each action's settings row with its binding's caps in a recorder's well, one action rebound with its Reset (no reference counterpart)",
            family: Family::Settings,
            client: SETTINGS_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("keyboard")],
        },
        Scenario {
            name: "settings-extensions",
            description: "Pane's Extensions page (#99): an installed package's management rows as list items, one unavailable here with its reason, its command and the install sources (no reference counterpart)",
            family: Family::Settings,
            client: SETTINGS_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("list")],
        },
        Scenario {
            name: "settings-extensions-confirm",
            description: "The Extensions page asking to confirm disabling a package another requires: its lines, then Disable all and Cancel (no reference counterpart)",
            family: Family::Settings,
            client: SETTINGS_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("confirm")],
        },
        Scenario {
            name: "settings-about",
            description: "Pane's About page (#99): the version, the update check's state and its button, the documentation and diagnostics buttons and what each reported (no reference counterpart)",
            family: Family::Settings,
            client: SETTINGS_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("about")],
        },
        Scenario {
            name: "form-validation",
            description: "The launcher's form after a rejected submission (#99): the name's well, focused, with its error under it, the greeting's segmented choice, the submit button and the footer's status (no reference counterpart)",
            family: Family::Form,
            client: ROOT_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("rejected")],
        },
        Scenario {
            name: "empty-state",
            description: "The empty board: 'kubectx' matches nothing, so the notice heads the fallbacks, none selected — the board preselects the first, which Pane never does (#100) — until Down selects it; the board's store suggestions are fixture content",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: true,
            board: Some("empty"),
            theme: None,
            frame: false,
            rows: EMPTY_ROWS,
            pins: &[],
            steps: &[capture("notice"), DOWN, capture("fallback-selected")],
        },
        Scenario {
            name: "calculator-card",
            description: "The calculator board: '72 in to cm' answered by the selected card, with the board's units, conversions and recent calculations as fixture content (Pane's calculator answers arithmetic only, #100)",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: true,
            board: Some("calculator"),
            theme: None,
            frame: false,
            rows: CALCULATOR_ROWS,
            pins: &[],
            steps: &[capture("card")],
        },
        Scenario {
            name: "answer-plain",
            description: "A computed answer as production presents it: '6*7' and 42 on the card under 'Calculator', no captions or chips (no reference counterpart)",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: ANSWER_ROWS,
            pins: &[],
            steps: &[capture("answer")],
        },
        Scenario {
            name: "answer-long",
            description: "A computed answer whose expression is too long for the authored 34px: the values step down to fit their columns (no reference counterpart)",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: LONG_ANSWER_ROWS,
            pins: &[],
            steps: &[capture("long-answer")],
        },
        Scenario {
            name: "empty-no-fallbacks",
            description: "The notice with no fallback under it, as production shows it to a user who chose none (no reference counterpart)",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("notice")],
        },
        Scenario {
            name: "clipboard-rest",
            description: "The clipboard board at rest: its fixture clips under Pinned, Today and Yesterday, the code clip selected and previewed (#102)",
            family: Family::Clipboard,
            client: CLIPBOARD_CLIENT,
            reference: true,
            board: Some("clipboard"),
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("rest")],
        },
        Scenario {
            name: "clipboard-previews",
            description: "A click selects a clip without copying it: the text, color, link and image previews in turn",
            family: Family::Clipboard,
            client: CLIPBOARD_CLIENT,
            reference: true,
            board: Some("clipboard"),
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[
                click("clip-standup"),
                capture("text"),
                click("clip-lime"),
                capture("color"),
                click("clip-link"),
                capture("link"),
                click("clip-shot"),
                capture("image"),
            ],
        },
        Scenario {
            name: "clipboard-keys",
            description: "Down moves the selection from the code clip; five more reach the last clip, which the list scrolls into view",
            family: Family::Clipboard,
            client: CLIPBOARD_CLIENT,
            reference: true,
            board: Some("clipboard"),
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[
                DOWN,
                capture("down"),
                DOWN,
                DOWN,
                DOWN,
                DOWN,
                DOWN,
                capture("last"),
            ],
        },
        Scenario {
            name: "clipboard-filter",
            description: "A query no clip matches: no preview and nothing selected; Escape clears it; the Text tab keeps the text clips",
            family: Family::Clipboard,
            client: CLIPBOARD_CLIENT,
            reference: true,
            board: Some("clipboard"),
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[
                Type { text: "zzz" },
                capture("no-match"),
                Key {
                    key: NamedKey::Escape,
                },
                capture("cleared"),
                click("clip-tab-text"),
                capture("text-tab"),
            ],
        },
        Scenario {
            name: "clipboard-production",
            description: "Production's own content: text records under Today, Yesterday and Older, the All and Text tabs, what is kept, Copy, Delete and Manage (no reference counterpart)",
            family: Family::Clipboard,
            client: CLIPBOARD_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("rest"), DOWN, DOWN, DOWN, capture("long-text")],
        },
        Scenario {
            name: "clipboard-off",
            description: "No records with history off: the note saying so, Turn on, no preview and no primary action (no reference counterpart)",
            family: Family::Clipboard,
            client: CLIPBOARD_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[capture("off")],
        },
        Scenario {
            name: "clipboard-narrow",
            description: "The split view in the launcher's own 760x518: the list keeps its width, the preview takes the rest, the keys reach the last record (no reference counterpart)",
            family: Family::Clipboard,
            client: ROOT_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: &[],
            pins: &[],
            steps: &[
                capture("narrow"),
                DOWN,
                DOWN,
                DOWN,
                DOWN,
                capture("narrow-last"),
            ],
        },
        Scenario {
            name: "pinned-strip",
            description: "The pinned home: five sample pins under the Pinned label above a blank query's rows; a query hides the strip and clearing it restores it; the pointer over slot 2 takes the slot's hover wash",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: true,
            board: None,
            theme: None,
            frame: false,
            rows: ROOT_ROWS,
            pins: ROOT_PINS,
            steps: &[
                capture("rest"),
                Type { text: "clip" },
                capture("query-hides"),
                Key {
                    key: NamedKey::Escape,
                },
                capture("cleared-restores"),
                point(SLOT_2),
                capture("slot-hover"),
            ],
        },
        Scenario {
            name: "pinned-partial",
            description: "A partly filled home: three pins, one whose target cannot run with its reason, and the pin hint in the cell after them (no reference counterpart)",
            family: Family::Root,
            client: ROOT_CLIENT,
            reference: false,
            board: None,
            theme: None,
            frame: false,
            rows: ROOT_ROWS,
            pins: PARTIAL_PINS,
            steps: &[capture("partial")],
        },
    ]
};

/// A reference board whose native scenarios do not exist yet, with the
/// ticket that will register them. Running one fails with this list, so
/// a missing scenario is never mistaken for a passing comparison.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingScenario {
    pub(crate) name: &'static str,
    /// The reference board the scenario captures.
    pub(crate) board: &'static str,
    pub(crate) ticket: &'static str,
    pub(crate) description: &'static str,
    pub(crate) client: (f32, f32),
}

/// The registered-but-pending scenarios: saved reference boards whose
/// native fixtures later tickets add. None is left: the Appearance page
/// (#98) was the last. Store and the snap HUD are deliberately absent —
/// they stay source-only fixture references for this milestone (see the
/// #90 specification's deferred capabilities).
pub(crate) fn pending_scenarios() -> &'static [PendingScenario] {
    &[]
}

/// One key sequence the keycap scenario renders: a production binding,
/// drawn through the launcher's own adapter
/// ([`crate::keyboard::binding_keys`]) in a cap style, and the reference
/// key group it pairs with, if the reference shows one.
pub(crate) struct KeyGroup {
    pub(crate) binding: &'static str,
    pub(crate) group: Option<&'static str>,
    pub(crate) style: CapStyle,
}

/// The keycap scenario's key sequences: the reference board's own chords
/// with its `platform` set to Windows — the footer's Actions Ctrl K, the
/// Left Half row's Win Alt ←, the Clipboard History row's Ctrl Shift V,
/// the footer's accent Enter and the first pinned slot's compact Ctrl 1 —
/// each paired by name with the reference's key group, so the comparison
/// sets the same effective binding beside each other; then chords the
/// reference never shows, which the primary action and a hint display
/// once the Keyboard page rebinds them (Shift+Enter, Ctrl+Enter,
/// Ctrl+Shift+P): captured, never compared.
pub(crate) const KEY_GROUPS: &[KeyGroup] = &[
    KeyGroup {
        binding: "ctrl-k",
        group: Some("footer-actions"),
        style: CapStyle::Regular,
    },
    KeyGroup {
        binding: "win-alt-left",
        group: Some("left-half"),
        style: CapStyle::Regular,
    },
    KeyGroup {
        binding: "ctrl-shift-v",
        group: Some("clipboard-history"),
        style: CapStyle::Regular,
    },
    KeyGroup {
        binding: "enter",
        group: Some("footer-primary"),
        style: CapStyle::Accent,
    },
    KeyGroup {
        binding: "ctrl-1",
        group: Some("pinned-1"),
        style: CapStyle::Compact,
    },
    KeyGroup {
        binding: "shift-enter",
        group: None,
        style: CapStyle::Accent,
    },
    KeyGroup {
        binding: "ctrl-enter",
        group: None,
        style: CapStyle::Accent,
    },
    KeyGroup {
        binding: "ctrl-shift-p",
        group: None,
        style: CapStyle::Regular,
    },
];

/// The tiles the tile scenario renders, left to right: each size as an
/// application (the reference's pen tone) and as a command, both with the
/// pen glyph — the same path at the application's 2px stroke and the
/// command's 1.6, so the comparison can weigh one against the other.
const TILES: &[(TileSize, IconTone, Glyph)] = &[
    (TileSize::Row, IconTone::Pen, Glyph::Pen),
    (TileSize::Row, IconTone::Command, Glyph::Pen),
    (TileSize::Slot, IconTone::Pen, Glyph::Pen),
    (TileSize::Slot, IconTone::Command, Glyph::Pen),
    (TileSize::Mini, IconTone::Pen, Glyph::Pen),
    (TileSize::Mini, IconTone::Command, Glyph::Pen),
];

/// The gap between the tile scenario's tiles.
const TILE_GAP: f32 = 24.;

/// One sidebar section of the reference Settings board, as the Settings
/// scenarios show it: the board's label and count, and the production
/// glyph drawn for it. `board_glyph` says whether that glyph is the
/// board's own path; the board's palette, shield and info glyphs have no
/// production counterpart, so those sections show a stand-in, compared by
/// place only.
#[derive(Debug, PartialEq)]
pub(crate) struct FixtureSection {
    pub(crate) label: &'static str,
    pub(crate) glyph: Glyph,
    pub(crate) board_glyph: bool,
    pub(crate) count: Option<&'static str>,
}

/// A section without a count.
const fn section(label: &'static str, glyph: Glyph, board_glyph: bool) -> FixtureSection {
    FixtureSection {
        label,
        glyph,
        board_glyph,
        count: None,
    }
}

/// The reference Settings board's sections, in its order, with its own
/// labels: the board is the reference's fixture data, not Pane's pages
/// (the Settings window's seven real pages are checked by its window
/// tests).
pub(crate) const SETTINGS_SECTIONS: &[FixtureSection] = &[
    section("General", Glyph::Sliders, true),
    section("Appearance", Glyph::Theme, false),
    section("Hotkeys & Aliases", Glyph::ActionHotkey, true),
    FixtureSection {
        count: Some("7"),
        ..section("Plugins", Glyph::Blocks, true)
    },
    section("Window Manager", Glyph::Layout, true),
    section("Clipboard", Glyph::Clipboard, true),
    section("Privacy", Glyph::Lock, false),
    section("About", Glyph::Gear, false),
];

/// The board's selected section: Appearance.
const SETTINGS_SELECTED: usize = 1;

/// The board's page: its heading, its subtitle and its preview column's
/// caption.
const SETTINGS_HEADING: &str = "Appearance";
const SETTINGS_SUBTITLE: &str =
    "Changes apply instantly. Themes from the Plugin Store show up here too.";
const SETTINGS_ASIDE: &str = "Preview";

/// Which Appearance page a Settings scenario draws, if any (#98).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppearancePage {
    /// The board's own page: its fixture content — every control the
    /// board shows, the advanced ones #100 defers among them — through the
    /// production control families.
    Board,
    /// Production's page, through the page's own composition
    /// (`features::settings::appearance::compose`): the theme and the
    /// material Pane has, offered — or, `overridden`, disabled under the
    /// run's development overrides, with the notice naming them.
    Production { overridden: bool },
}

/// The Appearance page the scenario `name` draws, if any.
pub(crate) fn appearance_page(name: &str) -> Option<AppearancePage> {
    match name {
        "appearance-page" | "appearance-solid" => Some(AppearancePage::Board),
        "appearance-production" | "appearance-light" | "appearance-narrow" => {
            Some(AppearancePage::Production { overridden: false })
        }
        "appearance-override" => Some(AppearancePage::Production { overridden: true }),
        _ => None,
    }
}

/// The board's materials, in segment order, each with the note the board
/// shows while it is chosen. Frost is the board's alone (#100).
pub(crate) const BOARD_MATERIALS: [(&str, &str); 3] = [
    ("Glass", "Dark, blurred glass that picks up your wallpaper."),
    ("Frost", "Lighter, brighter glass for pale wallpapers."),
    (
        "Solid",
        "No transparency. Pane switches to this when your OS asks to reduce transparency.",
    ),
];

/// The board's Solid, which dims its sliders (an index into
/// [`BOARD_MATERIALS`]).
const BOARD_SOLID: usize = 2;

/// The board's accent swatches (`0xRRGGBBAA`), Lime chosen.
pub(crate) const BOARD_SWATCHES: [(&str, u32); 4] = [
    ("Lime", 0xC9EE6AFF),
    ("Ice", 0x8FD3FFFF),
    ("Amber", 0xFFC46BFF),
    ("Lilac", 0xC3B2FFFF),
];

/// The board's sliders: label, the value it shows, and where the value
/// lies along the range (blur 44 of 0–60; tint 70 of 55–95).
const BOARD_BLUR: (&str, &str, f32) = ("Blur", "44 px", 44. / 60.);
const BOARD_TINT: (&str, &str, f32) = ("Tint", "70%", 15. / 40.);
const BOARD_TINT_NOTE: &str =
    "Tint never drops below 55% so text stays readable on bright wallpapers.";

/// The board's densities, Default chosen.
pub(crate) const BOARD_DENSITIES: [&str; 3] = ["Compact", "Default", "Roomy"];
const BOARD_DENSITY: usize = 1;

/// The board's toggles, both on.
pub(crate) const BOARD_TOGGLES: [&str; 2] = ["Show pinned slots", "Show tips in the footer"];

/// The link under the board's preview.
const BOARD_LINK: &str = "Find themes in the Plugin Store";

/// The board's miniature: its rows (title, kind, tile) with the first
/// selected, and its footer's tip. Its file row's tile is the board's own
/// slate tone, drawn as the neutral command tile.
const BOARD_PREVIEW_ROWS: [(&str, &str, IconTone, Glyph); 4] = [
    ("Figma", "Application", IconTone::Pen, Glyph::Pen),
    (
        "Configure Pane",
        "Command",
        IconTone::Command,
        Glyph::Sliders,
    ),
    ("figma-tokens.json", "File", IconTone::Command, Glyph::File),
    (
        "Recent Figma Files",
        "Command",
        IconTone::Command,
        Glyph::File,
    ),
];
const BOARD_TIP: &str = "Tab searches inside a plugin";

/// The segment a point or click step names: `segment-<label>`, lower case.
pub(crate) fn segment_target(label: &str) -> String {
    format!("segment-{}", label.to_lowercase())
}

/// The board's Frost and Solid segments, as steps name them.
pub(crate) const SEGMENT_FROST: &str = "segment-frost";
pub(crate) const SEGMENT_SOLID: &str = "segment-solid";

/// The board's miniature, with the effective invoke binding on its
/// action.
fn board_preview(keyboard: &pane_core::Keyboard) -> PreviewContent {
    PreviewContent {
        query: "fig".into(),
        pins: vec![
            (IconTone::Term, Glyph::Prompt),
            (IconTone::Code, Glyph::Code),
            (IconTone::Web, Glyph::Globe),
            (IconTone::Music, Glyph::Music),
        ],
        rows: BOARD_PREVIEW_ROWS
            .iter()
            .enumerate()
            .map(|(index, &(title, kind, tone, glyph))| PreviewRow {
                title: title.into(),
                kind: kind.into(),
                tone,
                glyph,
                selected: index == 0,
            })
            .collect(),
        tip: Some(BOARD_TIP.into()),
        action: "Open".into(),
        keys: crate::keyboard::binding_keys(keyboard.binding(KeyboardAction::InvokeSelectedAction)),
    }
}

/// What production's Appearance page shows in this run: the theme and
/// material in effect, and the descriptions of the run's overrides.
struct ProductionAppearance {
    theme: pane_core::ThemePreference,
    material: pane_core::MaterialPreference,
    overrides: Vec<String>,
    preview: PreviewContent,
    surface: Material,
}

impl ProductionAppearance {
    fn read(cx: &App) -> ProductionAppearance {
        let state = settings::shared(cx).read(cx);
        let keyboard = settings::keyboard_of(cx);
        let invoke = keyboard.binding(KeyboardAction::InvokeSelectedAction);
        let open_actions = keyboard.binding(KeyboardAction::OpenActions);
        ProductionAppearance {
            theme: state.theme_preference(),
            material: state.material_preference(),
            overrides: state.override_descriptions(),
            preview: {
                let _ = (invoke, open_actions);
                board_preview(&keyboard)
            },
            surface: settings::visuals(cx).material,
        }
    }
}

/// Production's Appearance page (#98), through the page's own
/// composition and parts, in the theme and material in effect: its
/// segments offered — or, `overridden`, under the notice naming the run's
/// overrides, disabled — but taking no clicks here (the window tests drive
/// the real page's).
fn render_appearance_production(
    overridden: bool,
    theme: &Theme,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let page = ProductionAppearance::read(cx);
    let offered = !overridden;
    let themes = appearance::THEMES
        .iter()
        .map(|&(preference, name, selector)| {
            controls::segment(name, preference == page.theme, offered, theme).id(selector)
        })
        .collect();
    let materials = appearance::MATERIALS
        .iter()
        .map(|&(preference, name, selector)| {
            controls::segment(name, preference == page.material, offered, theme).id(selector)
        })
        .collect();
    let view = appearance::AppearanceView {
        theme: page.theme,
        material: page.material,
        overrides: if overridden {
            page.overrides
        } else {
            Vec::new()
        },
        status: None,
    };
    div()
        .id("appearance")
        .child(appearance::compose(&view, (themes, materials), theme))
}

/// A deliberate fault the workbench injects to prove its comparison is
/// sensitive to exactly the errors the port cares about. The perturbed
/// fixture renders with one visual value wrong; the comparison must fail,
/// on the check that value drives, by the amount injected. The manifest
/// keeps declaring the unperturbed values — the perturbation is a fault
/// in the rendering, not a change of intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Perturbation {
    /// The rows' horizontal inset grows by 4 logical pixels — the result
    /// list's side padding, an edge error every wash, tile and text
    /// position shifts by. (The name predates the list's own padding
    /// token; the runner keeps passing it.)
    RowPaddingPlus4,
    /// The selected row's wash is filled with a wrong color — a regional
    /// fill error the wash's measured color reports.
    SelectedFill,
    /// The hover wash is filled with a wrong color, the same kind of fault
    /// on the pointer's state.
    HoverFill,
    /// The selected sidebar section's wash is filled with a wrong color:
    /// the Settings shell's own selection (#97).
    NavSelectedFill,
    /// The chosen segment's wash is filled with a wrong color: the
    /// Appearance page's segmented choice (#98).
    SegmentOnFill,
    /// A field's well is filled with a wrong color: the well every
    /// recorder, the select's trigger and a form's text sit in (#99).
    WellFill,
}

/// Every perturbation, by the name the runner passes.
const PERTURBATIONS: &[(&str, Perturbation)] = &[
    ("row-padding-plus-4", Perturbation::RowPaddingPlus4),
    ("selected-fill", Perturbation::SelectedFill),
    ("hover-fill", Perturbation::HoverFill),
    ("nav-selected-fill", Perturbation::NavSelectedFill),
    ("segment-on-fill", Perturbation::SegmentOnFill),
    ("well-fill", Perturbation::WellFill),
];

impl Perturbation {
    /// The perturbation `name` names, as the runner passes it.
    pub fn parse(name: &str) -> Result<Option<Perturbation>, String> {
        if name.is_empty() || name == "none" {
            return Ok(None);
        }
        PERTURBATIONS
            .iter()
            .find(|(known, _)| *known == name)
            .map(|&(_, perturbation)| Some(perturbation))
            .ok_or_else(|| {
                let known: Vec<_> = PERTURBATIONS.iter().map(|(known, _)| *known).collect();
                format!(
                    "unknown perturbation {name:?}; expected none or one of {}",
                    known.join(", ")
                )
            })
    }

    fn name(self) -> &'static str {
        PERTURBATIONS
            .iter()
            .find(|(_, perturbation)| *perturbation == self)
            .map(|(name, _)| *name)
            .expect("every perturbation is named")
    }

    /// `theme` with this fault applied: the frame renders a deliberately
    /// wrong value through the same production components.
    fn apply(self, theme: &mut Theme) {
        match self {
            Perturbation::RowPaddingPlus4 => theme.geometry.list_padding_x += px(4.),
            // A selected wash at white 25%: clearly wrong against the
            // authored 8.5%, and flat, so the measured fill is
            // deterministic in the opaque material.
            Perturbation::SelectedFill => {
                theme.row_selected = gpui::rgb_to_hsla(gpui::rgba(0xFFFFFF40))
            }
            // A hover wash at white 20% against the authored 3.5%.
            Perturbation::HoverFill => theme.row_hover = gpui::rgb_to_hsla(gpui::rgba(0xFFFFFF33)),
            // A selected section at white 25% against the authored 9%.
            Perturbation::NavSelectedFill => {
                theme.nav_selected = gpui::rgb_to_hsla(gpui::rgba(0xFFFFFF40))
            }
            // A chosen segment at white 30% against the authored 12%.
            Perturbation::SegmentOnFill => {
                theme.controls.segment_on = gpui::rgb_to_hsla(gpui::rgba(0xFFFFFF4D))
            }
            // A well at black 50% against the authored 24%.
            Perturbation::WellFill => theme.field_fill = gpui::rgb_to_hsla(gpui::rgba(0x00000080)),
        }
    }
}

/// How the fixture runs: the scenario to render, its isolated data
/// directory (or [`None`] to keep settings in memory), the theme and
/// material to render with (as `PANE_THEME`/`PANE_MATERIAL` name them),
/// the perturbation to inject, and where the manifest is written.
#[derive(Debug, PartialEq)]
pub struct FixtureOptions {
    pub scenario: String,
    pub data_dir: Option<PathBuf>,
    pub theme: Option<String>,
    pub material: Option<String>,
    pub perturbation: Option<Perturbation>,
    pub manifest: PathBuf,
}

/// What the fixture binary was asked to do.
#[derive(Debug, PartialEq)]
pub enum Command {
    /// Open the fixture window for a scenario.
    Run(FixtureOptions),
    /// Write the scenario registry (active and pending) as JSON to a
    /// file and exit: the capture helpers read the steps from it.
    Registry(PathBuf),
}

const USAGE: &str = "usage: pane-visual-fixture --scenario <name> --manifest <file> \
    [--data-dir <dir>] [--theme dark|light] [--material glass|opaque] \
    [--perturb none|row-padding-plus-4|selected-fill|hover-fill|nav-selected-fill|\
    segment-on-fill|well-fill]\n       \
    pane-visual-fixture --registry <file>";

/// Parses the fixture binary's arguments (without the program name).
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut args = args.into_iter();
    let mut scenario = None;
    let mut manifest = None;
    let mut data_dir = None;
    let mut theme = None;
    let mut material = None;
    let mut perturbation = None;
    let mut registry = None;
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))
        };
        match flag.as_str() {
            "--scenario" => scenario = Some(value()?),
            "--manifest" => manifest = Some(PathBuf::from(value()?)),
            "--data-dir" => data_dir = Some(PathBuf::from(value()?)),
            "--theme" => theme = Some(value()?),
            "--material" => material = Some(value()?),
            "--perturb" => perturbation = Perturbation::parse(&value()?)?,
            "--registry" => registry = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        }
    }
    if let Some(registry) = registry {
        return Ok(Command::Registry(registry));
    }
    let scenario = scenario.ok_or_else(|| format!("--scenario is required\n{USAGE}"))?;
    let manifest = manifest.ok_or_else(|| format!("--manifest is required\n{USAGE}"))?;
    Ok(Command::Run(FixtureOptions {
        scenario,
        data_dir,
        theme,
        material,
        perturbation,
        manifest,
    }))
}

/// Runs what `command` asks for.
pub fn execute(command: Command) -> Result<(), String> {
    match command {
        Command::Registry(path) => write_registry(&path),
        Command::Run(options) => run(options),
    }
}

/// The scenario `name`, or why there is none: a pending scenario names
/// the ticket that will register it.
fn find_scenario(name: &str) -> Result<&'static Scenario, String> {
    if let Some(scenario) = scenarios().iter().find(|scenario| scenario.name == name) {
        return Ok(scenario);
    }
    if let Some(pending) = pending_scenarios()
        .iter()
        .find(|pending| pending.name == name)
    {
        return Err(format!(
            "scenario {name:?} is pending: {} registers it ({})",
            pending.ticket, pending.description
        ));
    }
    Err(format!(
        "unknown scenario {name:?}; the workbench renders one of: {}",
        scenarios()
            .iter()
            .map(|scenario| scenario.name)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Registry {
    kind: &'static str,
    scenarios: &'static [Scenario],
    pending: &'static [PendingScenario],
}

fn write_registry(path: &Path) -> Result<(), String> {
    let registry = Registry {
        kind: "pane-visual-fixture-registry",
        scenarios: scenarios(),
        pending: pending_scenarios(),
    };
    write_json(path, &registry)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|error| format!("could not serialize {}: {error}", path.display()))?;
    std::fs::write(path, json)
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

/// The fixture's state: the rows the query shows and the selection. The
/// window drives it from real input; [`replay`] drives it from
/// a scenario's steps — the same transitions, so what the manifest
/// declares for a capture is what the window shows when it is taken.
#[derive(Clone, Debug)]
struct FixtureState {
    all: &'static [FixtureRow],
    /// The pinned home's pins, shown over a blank query.
    pins: &'static [FixturePin],
    query: String,
    rows: Vec<&'static FixtureRow>,
    /// The selected row's index; past the last row while none is: a list
    /// of fallbacks alone selects none until the user does, as root
    /// search's (#100).
    selected: usize,
    /// The Actions panel while it is open.
    actions: Option<PanelState>,
    /// The result board the scenario renders, if any (#96).
    board: Option<&'static ResultBoard>,
}

/// The open Actions panel's state: its filter and its selected entry.
#[derive(Clone, Debug, Default)]
struct PanelState {
    query: String,
    selected: usize,
}

impl FixtureState {
    fn new(all: &'static [FixtureRow], pins: &'static [FixturePin]) -> FixtureState {
        FixtureState {
            all,
            pins,
            query: String::new(),
            rows: all.iter().collect(),
            selected: 0,
            actions: None,
            board: None,
        }
    }

    /// `scenario`'s state at its rest: a result board's query is typed and
    /// its rows listed for it as authored, the first that is not a
    /// fallback selected (#96).
    fn of(scenario: &Scenario) -> FixtureState {
        let mut state = FixtureState::new(scenario.rows, scenario.pins);
        if let Some(board) = result_board(scenario.name) {
            state.board = Some(board);
            state.query = board.query.to_owned();
            state.selected = first_choice(&state.rows);
        }
        state
    }

    /// The board's answer, when the shown row `index` is drawn as its
    /// card: the board's first row.
    fn answer_at(&self, index: usize) -> Option<&'static FixtureAnswer> {
        let answer = self.board.and_then(|board| board.answer.as_ref());
        answer.filter(|_| index == 0)
    }

    /// Whether the shown row `index` is one of the board's calculation
    /// history rows: those after its card.
    fn history_at(&self, index: usize) -> bool {
        index > 0 && self.board.is_some_and(|board| board.history)
    }

    /// The board's extension suggestions, if it has any.
    fn suggestions(&self) -> Option<&'static FixtureSuggestions> {
        self.board.and_then(|board| board.suggestions.as_ref())
    }

    /// What the board's notice says for its query, if it has one: root
    /// search's own copy.
    fn notice(&self) -> Option<NoticeCopy> {
        let board = self.board.filter(|board| board.notice)?;
        let fallbacks = !self.rows.is_empty();
        Some(root_search::layouts::notice_copy(board.query, fallbacks))
    }

    /// The list child that shows the selected row (for scrolling to it):
    /// after the pinned home's children while it shows (#101), the
    /// board's notice, when it has one, and the labels.
    fn selected_child(&self) -> usize {
        let home = if self.home_shown() {
            pinned::HOME_CHILDREN
        } else {
            0
        };
        let notice = self.board.is_some_and(|board| board.notice);
        home + root_search::layouts::child_of_row(notice, &self.sections(), self.selected)
    }

    /// The footer's Actions button, as the launcher's opens and closes
    /// its panel.
    fn toggle_actions(&mut self) {
        self.actions = match self.actions {
            Some(_) => None,
            None => Some(PanelState::default()),
        };
    }

    /// The selected row's actions, or `None` with nothing selected.
    fn target_actions(&self) -> Option<ResultActions> {
        self.rows.get(self.selected).map(|row| row.actions())
    }

    /// What the open panel lists now: the target's actions its filter
    /// keeps, by the core's own rule.
    fn listed_actions(&self) -> Vec<ResultActionItem> {
        let (Some(panel), Some(actions)) = (&self.actions, self.target_actions()) else {
            return Vec::new();
        };
        actions
            .matching(&panel.query)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Text typed into the focused field: the panel's search while the
    /// panel is open, else the query.
    fn type_text(&mut self, text: &str) {
        match self.actions.as_mut() {
            Some(panel) => {
                panel.query.push_str(text);
                panel.selected = 0;
            }
            None => {
                let query = format!("{}{text}", self.query);
                self.set_query(&query);
            }
        }
    }

    /// Down: through the panel's entries that can run while it is open,
    /// as the launcher's panel moves, else the results.
    fn down(&mut self) {
        let listed = self.listed_actions();
        match self.actions.as_mut() {
            Some(panel) => {
                if let Some(next) = actions_panel::next_available(&listed, panel.selected, true) {
                    panel.selected = next;
                }
            }
            None => self.select_next(),
        }
    }

    /// Escape: it closes the panel while the panel is open, and only
    /// that; else the launcher's Back.
    fn escape(&mut self) {
        if self.actions.take().is_none() {
            self.back();
        }
    }

    /// The reference's own filtering rule, over the fixture's own data: a
    /// row shows when its title contains the query, ignoring case; a new
    /// query selects its first result that is not a fallback, as the
    /// launcher's does.
    fn set_query(&mut self, query: &str) {
        let needle = query.to_lowercase();
        self.query = query.to_owned();
        self.rows = self
            .all
            .iter()
            .filter(|row| row.title.to_lowercase().contains(&needle))
            .collect();
        self.selected = first_choice(&self.rows);
    }

    fn select_next(&mut self) {
        if self.selected >= self.rows.len() {
            // Nothing is selected (fallbacks alone): Down chooses the
            // first, as root search's does.
            if !self.rows.is_empty() {
                self.selected = 0;
            }
        } else if self.selected + 1 < self.rows.len() {
            self.selected += 1;
        }
    }

    fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Escape, as root search's own Back behaves on a query: the query
    /// clears and the selection returns to the first row.
    fn back(&mut self) {
        self.set_query("");
    }

    /// The pointer moved over the shown row `row`: root search's rows
    /// select under the moving pointer (#94).
    fn pointer_over(&mut self, row: usize) {
        if row < self.rows.len() {
            self.selected = row;
        }
    }

    /// Whether the pinned home shows: the scenario has one, and the
    /// trimmed query is blank, as the launcher's rule is
    /// (`features::quick_slots::home_shown`).
    fn home_shown(&self) -> bool {
        !self.pins.is_empty() && self.query.trim().is_empty()
    }

    /// The section labels over the shown rows, by root search's own rule
    /// (`pane_core::answer_sections`: a board's answer under its command's
    /// title, the fallbacks under "Fallbacks"), or the labels a result
    /// board authors where they are its content (#96).
    fn sections(&self) -> Vec<SectionLabel> {
        if let Some(sections) = self.board.and_then(|board| board.sections) {
            return sections
                .iter()
                .map(|&(first, label, note)| SectionLabel {
                    first,
                    label: label.into(),
                    note: Some(note.into()),
                })
                .collect();
        }
        let rows = self.rows.len();
        let fallbacks = self
            .rows
            .iter()
            .position(|row| row.kind == "Fallback")
            .unwrap_or(rows);
        let answers: Vec<Option<&str>> = (0..rows)
            .map(|index| self.answer_at(index).map(|answer| answer.command))
            .collect();
        pane_core::answer_sections(&self.query, &answers, fallbacks)
            .iter()
            .map(crate::app::section_label)
            .collect()
    }
}

/// The row a list selects by itself: its first that is not a fallback, as
/// root search's (`pane_core`'s first choice); past the last row when it
/// lists fallbacks alone, which wait for the user (#100).
fn first_choice(rows: &[&FixtureRow]) -> usize {
    rows.iter()
        .position(|row| row.kind != "Fallback")
        .unwrap_or(rows.len())
}

/// A rectangle in logical client coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(crate) struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Rect {
    fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2., self.y + self.height / 2.)
    }

    fn contains(&self, (x, y): (f32, f32)) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// The geometry the root composition declares in a client of `client`
/// size: the search header, the list's viewport and the footer, edge to
/// edge — the panel's inset ring takes no layout space.
struct Frame {
    search: Rect,
    list: Rect,
    footer: Rect,
}

fn frame(theme: &Theme, client: (f32, f32)) -> Frame {
    let geometry = &theme.geometry;
    let (width, height) = client;
    let search = Rect {
        x: 0.,
        y: 0.,
        width,
        height: f32::from(geometry.search_height),
    };
    let footer = Rect {
        x: 0.,
        y: height - f32::from(geometry.footer_height),
        width,
        height: f32::from(geometry.footer_height),
    };
    let list = Rect {
        x: 0.,
        y: search.y + search.height,
        width,
        height: footer.y - (search.y + search.height),
    };
    Frame {
        search,
        list,
        footer,
    }
}

/// One row as a capture declares it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredRow {
    title: &'static str,
    /// "app" for a gradient tile, "command" for the neutral one.
    tone: &'static str,
    subtitle: Option<&'static str>,
    unavailable: Option<&'static str>,
    selected: bool,
    hovered: bool,
    /// The row's rect. A row holding an unavailable reason grows past its
    /// height floor by the wrapped reason, which the fixture cannot
    /// declare without shaping text, so it and every row after it declare
    /// `heightIsFloor` and only the first such row's top is exact.
    rect: Rect,
    height_is_floor: bool,
    /// Whether the whole row lies inside the list's viewport at the
    /// list's scroll offset; a row scrolled out of view, or partly out,
    /// is declared but not measured.
    visible: bool,
    kind: &'static str,
    alias: Option<&'static str>,
    keys: Option<&'static str>,
    /// Where the query matched the title (byte ranges), by the launcher's
    /// own rule (`pane_core::title_matches`).
    matched: Vec<std::ops::Range<usize>>,
    /// Where the row's trailing parts lie, filled in from shaped text
    /// when the manifest is written.
    #[serde(skip_serializing_if = "Option::is_none")]
    kind_rect: Option<Rect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alias_rect: Option<Rect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_group: Option<KeyGroupRecord>,
    /// Whether the row is drawn as the board's answer card (#96): its
    /// rect is the card's, and the rows' own checks pass it by.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    answer: bool,
}

/// A section label as a capture declares it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredSection {
    label: String,
    note: Option<String>,
    rect: Rect,
}

/// The pinned home as a capture declares it: the "Pinned" label, the
/// strip, its slots and the pin hint after them, laid out from the theme's
/// tokens as `crate::ui::pinned` draws them.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredPinned {
    label: Rect,
    /// The label's chord ("Ctrl" "1–5"), filled in from shaped text when
    /// the manifest is written.
    #[serde(skip_serializing_if = "Option::is_none")]
    label_keys: Option<KeyGroupRecord>,
    strip: Rect,
    slots: Vec<DeclaredSlot>,
    /// The pin hint's cell, after the last slot, while it shows (see
    /// `crate::ui::pinned::shows_pin_hint`).
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<Rect>,
}

/// One quick slot as a capture declares it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredSlot {
    /// Its place, counting from 1.
    number: usize,
    /// What it holds.
    title: &'static str,
    /// "app" for a gradient tile, "command" for the neutral one.
    tone: &'static str,
    unavailable: Option<&'static str>,
    rect: Rect,
    /// Its 42px tile, centered across the slot.
    tile: Rect,
    /// Its title's line, inside the slot's side padding; the text is
    /// centered in it.
    title_rect: Rect,
    /// Whether the pointer is over it (it takes the hover wash).
    hovered: bool,
    /// Its compact chord in its top right corner, filled in from shaped
    /// text when the manifest is written.
    #[serde(skip_serializing_if = "Option::is_none")]
    key_group: Option<KeyGroupRecord>,
    /// Whether its title is wider than its line and so ends in an
    /// ellipsis (its ink is then not centered), from shaped text when the
    /// manifest is written.
    #[serde(skip_serializing_if = "Option::is_none")]
    truncated: Option<bool>,
}

/// The height of the strip of `pins` pins, its paddings included: its
/// rows of slots ([`pinned::strip_rows`]), the columns' gap between them.
fn strip_height(theme: &Theme, pins: usize) -> f32 {
    let pinned = &theme.geometry.pinned;
    let rows = pinned::strip_rows(pins) as f32;
    f32::from(pinned.strip_padding_top)
        + rows * f32::from(pinned.slot_height)
        + (rows - 1.) * f32::from(pinned.columns_gap)
        + f32::from(pinned.strip_padding_bottom)
}

/// The height the pinned home of `pins` pins takes above the rows, the
/// list's gap after it included: the label, the gap, the strip with its
/// paddings, the gap.
fn home_height(theme: &Theme, pins: usize) -> f32 {
    let geometry = &theme.geometry;
    f32::from(geometry.section_height)
        + strip_height(theme, pins)
        + 2. * f32::from(geometry.row_list_gap)
}

/// How many of `pins` pins Ctrl and a digit number, as the launcher's
/// home numbers them (`features::quick_slots::NUMBERED_PINS`).
fn numbered_pins(pins: usize) -> usize {
    pins.min(crate::features::quick_slots::NUMBERED_PINS)
}

/// "app" for a gradient tone, "command" for the neutral tile.
fn tone_name(tone: IconTone) -> &'static str {
    if tone == IconTone::Command {
        "command"
    } else {
        "app"
    }
}

/// The pinned home `state` shows, at the top of the list of `frame`
/// scrolled by `offset`, with the pointer at `pointer`; `None` while it
/// does not show.
fn declared_home(
    state: &FixtureState,
    theme: &Theme,
    frame: &Frame,
    offset: f32,
    pointer: Option<(f32, f32)>,
) -> Option<DeclaredPinned> {
    if !state.home_shown() {
        return None;
    }
    let f = f32::from;
    let geometry = &theme.geometry;
    let typography = &theme.typography;
    let pinned = &geometry.pinned;
    let x = frame.list.x + f(geometry.list_padding_x);
    let width = frame.list.width - 2. * f(geometry.list_padding_x);
    let top = frame.list.y + f(geometry.list_padding_top) + offset;
    let label = Rect {
        x,
        y: top,
        width,
        height: f(geometry.section_height),
    };
    let strip = Rect {
        x,
        y: top + f(geometry.section_height) + f(geometry.row_list_gap),
        width,
        height: strip_height(theme, state.pins.len()),
    };
    // The strip's equal columns, its cells filling them row by row, the
    // columns' gap between the rows too.
    let columns = pinned::PINNED_COLUMNS as f32;
    let gap = f(pinned.columns_gap);
    let column = (width - gap * (columns - 1.)) / columns;
    let cell = |index: usize| Rect {
        x: x + (index % pinned::PINNED_COLUMNS) as f32 * (column + gap),
        y: strip.y
            + f(pinned.strip_padding_top)
            + (index / pinned::PINNED_COLUMNS) as f32 * (f(pinned.slot_height) + gap),
        width: column,
        height: f(pinned.slot_height),
    };
    let tile = f(geometry.slot_tile.size);
    let line = f(typography.slot_title_size) * typography.line_height;
    let reason_line = f(typography.slot_reason_size) * typography.line_height;
    let inner = f(pinned.slot_height) - f(pinned.slot_padding_top) - f(pinned.slot_padding_bottom);
    let slots = state
        .pins
        .iter()
        .enumerate()
        .map(|(index, pin)| {
            let rect = cell(index);
            // The tile and the title, centered down the slot inside its
            // paddings, the slot's gap between them; an unavailable slot's
            // reason right under its title, the tile closer to them.
            let reason = pin.unavailable;
            let text_gap = f(if reason.is_some() {
                pinned.unavailable_gap
            } else {
                pinned.slot_gap
            });
            let content = tile + text_gap + line + reason.map_or(0., |_| reason_line);
            let tile_y = rect.y + f(pinned.slot_padding_top) + (inner - content) / 2.;
            DeclaredSlot {
                number: index + 1,
                title: pin.title,
                tone: tone_name(pin.icon.0),
                unavailable: reason,
                rect,
                tile: Rect {
                    x: rect.x + (column - tile) / 2.,
                    y: tile_y,
                    width: tile,
                    height: tile,
                },
                title_rect: Rect {
                    x: rect.x + f(pinned.slot_padding_x),
                    y: tile_y + tile + text_gap,
                    width: column - 2. * f(pinned.slot_padding_x),
                    height: line,
                },
                hovered: pointer.is_some_and(|point| rect.contains(point)),
                key_group: None,
                truncated: None,
            }
        })
        .collect();
    let hint = pinned::shows_pin_hint(state.pins.len()).then(|| cell(state.pins.len()));
    Some(DeclaredPinned {
        label,
        label_keys: None,
        strip,
        slots,
        hint,
    })
}

/// The rows `state` shows, laid out in the list of `frame` scrolled by
/// `offset` (0 or negative, as GPUI's scroll offset is), with the pointer
/// at `pointer` (client coordinates) if it is in the window.
fn declared_rows(
    state: &FixtureState,
    theme: &Theme,
    frame: &Frame,
    offset: f32,
    pointer: Option<(f32, f32)>,
) -> Vec<DeclaredRow> {
    declared_list(state, theme, frame, offset, pointer).0
}

/// The rows and the section labels `state` shows, laid out down the list
/// (see [`declared_rows`]): each label ahead of its first row, a label's
/// height and the list's gap before it.
fn declared_list(
    state: &FixtureState,
    theme: &Theme,
    frame: &Frame,
    offset: f32,
    pointer: Option<(f32, f32)>,
) -> (Vec<DeclaredRow>, Vec<DeclaredSection>) {
    let geometry = &theme.geometry;
    let x = frame.list.x + f32::from(geometry.list_padding_x);
    let width = frame.list.width - 2. * f32::from(geometry.list_padding_x);
    let mut y = frame.list.y + f32::from(geometry.list_padding_top) + offset;
    // A result board's notice heads the list (#96).
    if state.board.is_some_and(|board| board.notice) {
        y += f32::from(geometry.results.notice_height) + f32::from(geometry.row_list_gap);
    }
    let list = frame.list;
    let mut floor = false;
    let sections = state.sections();
    let mut labels = Vec::new();
    // The pinned home above the rows, its label first among the labels,
    // its note the caps of its chord as the reference's DOM reads them
    // ("Ctrl1–5").
    if let Some(home) = declared_home(state, theme, frame, offset, None) {
        let keys = crate::keyboard::quick_slots_keys(numbered_pins(state.pins.len()));
        labels.push(DeclaredSection {
            label: pinned::PINNED_LABEL.to_owned(),
            note: Some(keys.keys.iter().map(|key| key.cap.as_ref()).collect()),
            rect: home.label,
        });
        y += home_height(theme, state.pins.len());
    }
    let rows = state
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            for section in sections.iter().filter(|section| section.first == index) {
                labels.push(DeclaredSection {
                    label: section.label.to_string(),
                    note: section.note.as_ref().map(ToString::to_string),
                    rect: Rect {
                        x,
                        y,
                        width,
                        height: f32::from(geometry.section_height),
                    },
                });
                y += f32::from(geometry.section_height) + f32::from(geometry.row_list_gap);
            }
            floor |= row.unavailable.is_some();
            // A row drawn as the board's answer card is the card, between
            // its margins (#96).
            let answer = state.answer_at(index);
            let (above, height, below) = match answer {
                Some(answer) => (
                    f32::from(geometry.results.card_margin_top),
                    answer_card_height(&answer.card(false), theme),
                    f32::from(geometry.results.card_margin_bottom),
                ),
                None => (0., f32::from(geometry.row_min_height), 0.),
            };
            let rect = Rect {
                x,
                y: y + above,
                width,
                height,
            };
            y += above + height + below + f32::from(geometry.row_list_gap);
            DeclaredRow {
                title: row.title,
                tone: if row.icon.0 == IconTone::Command {
                    "command"
                } else {
                    "app"
                },
                subtitle: row.subtitle,
                unavailable: row.unavailable,
                selected: index == state.selected,
                hovered: !floor && pointer.is_some_and(|point| rect.contains(point)),
                rect,
                height_is_floor: floor,
                visible: rect.y >= list.y && rect.y + rect.height <= list.y + list.height,
                kind: row.kind,
                alias: row.alias,
                keys: row.keys,
                matched: pane_core::title_matches(row.title, &state.query),
                kind_rect: None,
                alias_rect: None,
                key_group: None,
                answer: answer.is_some(),
            }
        })
        .collect();
    (rows, labels)
}

/// One capture a scenario takes, as the fixture declares it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredCapture {
    name: &'static str,
    /// The steps taken since the scenario's rest, in order.
    after: Vec<Step>,
    query: String,
    /// The pointer's point, in logical client coordinates, while the
    /// capture is taken — the cursor's own glyph paints there, so the
    /// comparison masks a small region around it (never the control
    /// under test, which it measures beside the cursor).
    pointer: Option<(f32, f32)>,
    rows: Vec<DeclaredRow>,
    /// The section labels over the rows.
    sections: Vec<DeclaredSection>,
    /// The footer's primary action label: the selected row's.
    action: Option<&'static str>,
    /// The footer action button's rect, filled in from the shaped label
    /// when the manifest is written (the replay itself shapes no text).
    #[serde(skip_serializing_if = "Option::is_none")]
    action_button: Option<Rect>,
    /// Whether the Actions panel is open.
    actions_open: bool,
    /// The open Actions panel's layout.
    #[serde(skip_serializing_if = "Option::is_none")]
    actions: Option<DeclaredActions>,
    /// The footer's mark, hint and buttons, filled in from shaped text
    /// when the manifest is written.
    #[serde(skip_serializing_if = "Option::is_none")]
    footer: Option<DeclaredFooter>,
    /// A result board's own parts (#96), filled in from shaped text when
    /// the manifest is written.
    #[serde(skip_serializing_if = "Option::is_none")]
    board: Option<DeclaredBoard>,
    /// The split view, in a clipboard scenario's captures (#102).
    #[serde(skip_serializing_if = "Option::is_none")]
    clipboard: Option<DeclaredClipboard>,
    /// The pinned home, while it shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pinned: Option<DeclaredPinned>,
}

/// A result board's own parts as a capture declares them (#96): the
/// notice, the answer card and the suggestions, each where the shared
/// result layouts lay it out, with the colors it is drawn in.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredBoard {
    #[serde(skip_serializing_if = "Option::is_none")]
    notice: Option<DeclaredNotice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    answer: Option<DeclaredAnswer>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    suggestions: Vec<DeclaredSuggestion>,
}

/// The no-results notice: its box, its disc and the disc's fill, and its
/// title's and description's line boxes.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredNotice {
    rect: Rect,
    disc: Rect,
    disc_fill: String,
    title: DeclaredText,
    description: DeclaredText,
}

/// The answer card: its box, whether its accent ring shows, its fill, the
/// values' type, each side, the arrow's disc, and the "Also" line — its
/// label, its chips and the rule above them — where it has one.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredAnswer {
    rect: Rect,
    selected: bool,
    fill: String,
    ring: String,
    /// The values' type: "34px Geist Mono 500".
    font: String,
    source: DeclaredSide,
    answer: DeclaredSide,
    arrow: Rect,
    arrow_fill: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    also: Option<DeclaredText>,
    chips: Vec<DeclaredText>,
    chip_fill: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rule: Option<Rect>,
}

/// One side of the answer card: its value's box (shaped, centered in its
/// column) and color, and its caption's box.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredSide {
    value: DeclaredText,
    color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    caption: Option<DeclaredText>,
}

/// An extension suggestion row: its box, its tile and its pill, and the
/// pill's fill.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredSuggestion {
    title: String,
    rect: Rect,
    tile: Rect,
    pill: Rect,
    pill_fill: String,
}

/// The open Actions panel as a capture declares it, laid out from the
/// panel's tokens (see [`declared_actions`]).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredActions {
    /// The panel's own box.
    rect: Rect,
    /// The header, naming the target; none with nothing selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    header: Option<DeclaredActionsHeader>,
    rows: Vec<DeclaredActionRow>,
    /// The group labels ("Pane").
    groups: Vec<DeclaredText>,
    /// The separators' 1px rules.
    rules: Vec<Rect>,
    /// The note shown in place of entries, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    empty: Option<DeclaredText>,
    /// The search row, its rule along its top edge.
    search: Rect,
    /// The dimmer over the results: the list's area.
    dimmer: Rect,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredActionsHeader {
    title: String,
    rect: Rect,
    /// The target's 18px tile.
    tile: Rect,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredActionRow {
    label: String,
    /// "invoke", "hotkey" or "alias".
    action: &'static str,
    rect: Rect,
    selected: bool,
    /// The entry's 16px glyph box.
    glyph: Rect,
}

/// A piece of text a capture declares: what it says and where its box is.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredText {
    text: String,
    rect: Rect,
}

/// The footer as a capture declares it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredFooter {
    /// The Pane mark's 18px box.
    mark: Rect,
    /// The hint's parts, left to right.
    hint: Vec<DeclaredHintPart>,
    /// The rule between the buttons.
    divider: Rect,
    /// The Actions button, and whether it shows its panel open.
    actions_button: Rect,
    actions_pressed: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredHintPart {
    /// The text, or the key sequence's caps' labels.
    text: String,
    keys: bool,
    /// Its horizontal extent, across the strip's first line.
    rect: Rect,
    /// Whether the part runs past the room the buttons leave the hint (a
    /// narrow window), where the hint is clipped.
    clipped: bool,
}

/// The open Actions panel's layout over `state`, as the production panel
/// draws it (see `crate::features::actions_panel`): its right edge 10px in
/// from the window's and its bottom 8px above the footer; inside, the
/// header, the list — entries, the group's rule and label unless the
/// filter narrows them, or the note — and the search row.
fn declared_actions(state: &FixtureState, theme: &Theme, frame: &Frame) -> Option<DeclaredActions> {
    let panel = state.actions.as_ref()?;
    let geometry = &theme.geometry.actions;
    let f = f32::from;
    let width = f(geometry.width);
    let x = frame.footer.x + frame.footer.width - f(geometry.inset) - width;
    let target = state.target_actions();
    let listed = state.listed_actions();
    let filtering = !panel.query.trim().is_empty();
    // Laid out from the panel's top at 0, then moved into place.
    let mut y = 0.;
    let header = target.as_ref().map(|actions| {
        let mini = f(theme.geometry.mini_tile.size);
        let content = f(geometry.header_height) - f(geometry.header_padding_top);
        DeclaredActionsHeader {
            title: actions.title.clone(),
            rect: Rect {
                x,
                y: 0.,
                width,
                height: f(geometry.header_height),
            },
            tile: Rect {
                x: x + f(geometry.header_padding_x),
                y: f(geometry.header_padding_top) + (content - mini) / 2.,
                width: mini,
                height: mini,
            },
        }
    });
    if header.is_some() {
        y += f(geometry.header_height);
    }
    y += f(geometry.list_padding);
    let inner_x = x + f(geometry.list_padding);
    let inner_width = width - 2. * f(geometry.list_padding);
    let gap = f(geometry.list_gap);
    // The list's children, top to bottom, with the gap between them.
    let mut children = 0;
    let mut child = |y: &mut f32| {
        if children > 0 {
            *y += gap;
        }
        children += 1;
    };
    let (mut rows, mut groups, mut rules) = (Vec::new(), Vec::new(), Vec::new());
    for panel_child in actions_panel::panel_children(&listed, filtering) {
        child(&mut y);
        match panel_child {
            actions_panel::PanelChild::Rule => {
                rules.push(Rect {
                    x: inner_x + f(geometry.rule_margin_x),
                    y: y + f(geometry.rule_margin_y),
                    width: inner_width - 2. * f(geometry.rule_margin_x),
                    height: 1.,
                });
                y += 1. + 2. * f(geometry.rule_margin_y);
            }
            actions_panel::PanelChild::Group => {
                groups.push(DeclaredText {
                    text: actions_panel::PANE_GROUP.to_owned(),
                    rect: Rect {
                        x: inner_x,
                        y,
                        width: inner_width,
                        height: f(geometry.group_height),
                    },
                });
                y += f(geometry.group_height);
            }
            actions_panel::PanelChild::Entry(index) => {
                let item = &listed[index];
                let glyph = f(geometry.glyph_size);
                rows.push(DeclaredActionRow {
                    label: item.label.clone(),
                    action: item.action.id(),
                    rect: Rect {
                        x: inner_x,
                        y,
                        width: inner_width,
                        height: f(geometry.row_height),
                    },
                    selected: index == panel.selected,
                    glyph: Rect {
                        x: inner_x + f(geometry.row_padding_x),
                        y: y + (f(geometry.row_height) - glyph) / 2.,
                        width: glyph,
                        height: glyph,
                    },
                });
                y += f(geometry.row_height);
            }
        }
    }
    let note = match (&target, listed.is_empty()) {
        (None, _) => Some(actions_panel::NOTHING_SELECTED),
        (Some(_), true) => Some(actions_panel::NO_MATCH),
        (Some(_), false) => None,
    };
    let empty = note.map(|text| {
        child(&mut y);
        let height = 2. * f(geometry.empty_padding_y)
            + f(theme.typography.action_size) * theme.typography.line_height;
        let rect = Rect {
            x: inner_x,
            y,
            width: inner_width,
            height,
        };
        y += height;
        DeclaredText {
            text: text.to_owned(),
            rect,
        }
    });
    y += f(geometry.list_padding);
    let search = Rect {
        x,
        y,
        width,
        height: f(geometry.search_height),
    };
    y += f(geometry.search_height);
    // Into place: the panel's bottom above the footer.
    let top = frame.footer.y - f(geometry.above_footer) - y;
    let place = |rect: Rect| Rect {
        y: rect.y + top,
        ..rect
    };
    Some(DeclaredActions {
        rect: place(Rect {
            x,
            y: 0.,
            width,
            height: y,
        }),
        header: header.map(|header| DeclaredActionsHeader {
            rect: place(header.rect),
            tile: place(header.tile),
            ..header
        }),
        rows: rows
            .into_iter()
            .map(|row| DeclaredActionRow {
                rect: place(row.rect),
                glyph: place(row.glyph),
                ..row
            })
            .collect(),
        groups: groups
            .into_iter()
            .map(|group| DeclaredText {
                rect: place(group.rect),
                ..group
            })
            .collect(),
        rules: rules.into_iter().map(place).collect(),
        empty: empty.map(|empty| DeclaredText {
            rect: place(empty.rect),
            ..empty
        }),
        search: place(search),
        dimmer: frame.list,
    })
}

/// A step with the point it acts at, resolved: the client point a
/// pointer step moves to, in logical client coordinates.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ResolvedStep {
    #[serde(flatten)]
    step: Step,
    #[serde(skip_serializing_if = "Option::is_none")]
    point: Option<(f32, f32)>,
}

/// A scenario replayed over the fixture's state model: every capture it
/// takes and every step it takes, resolved.
pub(crate) struct Replay {
    pub(crate) captures: Vec<DeclaredCapture>,
    pub(crate) steps: Vec<ResolvedStep>,
}

/// The list's scroll offset after it scrolls the selected row into view
/// from `offset`, by the rule GPUI's `ScrollHandle::scroll_to_item`
/// applies (the least scroll that shows the whole row; the list's own
/// padding may scroll out of view), which both the launcher and the
/// fixture ask for whenever the selection moves.
fn scrolled_to_selected(state: &FixtureState, theme: &Theme, frame: &Frame, offset: f32) -> f32 {
    let rows = declared_rows(state, theme, frame, 0., None);
    let Some(row) = rows.get(state.selected) else {
        return offset;
    };
    let (top, bottom) = (frame.list.y, frame.list.y + frame.list.height);
    if row.rect.y + offset < top {
        top - row.rect.y
    } else if row.rect.y + row.rect.height + offset > bottom {
        bottom - (row.rect.y + row.rect.height)
    } else {
        offset
    }
}

/// Replays `scenario`'s steps over the fixture's state model: declares
/// every capture they take, and resolves where each pointer step points.
pub(crate) fn replay(scenario: &Scenario, theme: &Theme) -> Replay {
    if scenario.family == Family::Clipboard {
        return replay_clipboard(scenario, theme);
    }
    let frame = frame(theme, scenario.client);
    let mut state = FixtureState::of(scenario);
    let mut offset = 0.;
    let mut pointer = None;
    let mut after = Vec::new();
    let mut captures = Vec::new();
    let mut steps = Vec::new();
    for step in scenario.steps {
        let mut point = None;
        match *step {
            Step::Capture { name } => {
                let (rows, sections) = declared_list(&state, theme, &frame, offset, pointer);
                captures.push(DeclaredCapture {
                    name,
                    after: after.clone(),
                    query: state.query.clone(),
                    pointer,
                    rows,
                    sections,
                    action: state.rows.get(state.selected).map(|row| row.action),
                    action_button: None,
                    actions_open: state.actions.is_some(),
                    actions: declared_actions(&state, theme, &frame),
                    footer: None,
                    board: None,
                    clipboard: None,
                    pinned: declared_home(&state, theme, &frame, offset, pointer),
                })
            }
            // The point is the slot's center, which the layout places.
            Step::Point { target } => {
                point = declared_home(&state, theme, &frame, offset, None).and_then(|home| {
                    home.slots
                        .iter()
                        .find(|slot| format!("slot-{}", slot.number) == target)
                        .map(|slot| slot.rect.center())
                });
                pointer = point;
            }
            Step::Pointer { row, nudge } => {
                let rows = declared_rows(&state, theme, &frame, offset, None);
                let moved_to = rows.get(row).map(|shown| {
                    let (x, y) = shown.rect.center();
                    (x + f32::from(nudge), y)
                });
                // The capture helpers move the pointer onto the row from a
                // pixel to its left, as a real pointer arrives: the first
                // event records where it is, the second is movement and
                // selects the row. A step that lands where the pointer
                // already is moves nothing.
                if moved_to.is_some() && moved_to != pointer {
                    state.pointer_over(row);
                }
                pointer = moved_to;
                point = pointer;
            }
            Step::Key {
                key: NamedKey::Down,
            } => state.down(),
            Step::Key {
                key: NamedKey::Escape,
            } => state.escape(),
            Step::Type { text } => state.type_text(text),
            // The point is the button's center, which shaping places: the
            // manifest fills it in.
            Step::Click { target } => {
                if target == ACTIONS_BUTTON {
                    state.toggle_actions();
                }
            }
        }
        if matches!(
            step,
            Step::Key { .. } | Step::Type { .. } | Step::Pointer { .. }
        ) {
            offset = scrolled_to_selected(&state, theme, &frame, offset);
        }
        if !matches!(step, Step::Capture { .. }) {
            after.push(*step);
        }
        steps.push(ResolvedStep { step: *step, point });
    }
    Replay { captures, steps }
}

/// The fixture window: the scenario's state over the production
/// components. It owns the query field's editable text entity (the same
/// element the launcher's fields use) and the fixture's state; the
/// selection keys, the typing and the pointer act on it exactly as the
/// capture helper drives them, through the production bindings the
/// launcher itself registers.
pub(crate) struct FixtureWindow {
    scenario: &'static Scenario,
    perturbation: Option<Perturbation>,
    state: FixtureState,
    query: Entity<EditableTextState>,
    /// The result list's scroll, kept on the selected row as the
    /// launcher keeps its own (see [`scrolled_to_selected`]).
    scroll: ScrollHandle,
    /// Where the pointer last moved, as the launcher keeps it: only real
    /// movement over a row selects it.
    pointer: Option<gpui::Point<Pixels>>,
    /// The Actions panel's search field (the panel's state is the
    /// fixture state's).
    filter: Entity<EditableTextState>,
    /// A clipboard scenario's split view (#102): its records, query, tab
    /// and selection; the query field above is its search.
    clip: Option<ClipState>,
    /// The Appearance board's chosen material, an index into
    /// [`BOARD_MATERIALS`] (#98): a click on a segment changes it.
    board_material: usize,
    /// The Launcher page's select (#99): the production control, over
    /// the fixture's display layout.
    select: Option<Entity<crate::ui::select::Select>>,
}

impl FixtureWindow {
    fn new(
        scenario: &'static Scenario,
        perturbation: Option<Perturbation>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> FixtureWindow {
        // A result board's query is in the field from the start (#96),
        // its caret after it, as typing it leaves the caret.
        let state = FixtureState::of(scenario);
        let text = StringStorage::from(state.query.clone());
        let typed = state.query.len();
        let query = cx.new(|cx| {
            let mut field = EditableTextState::new(text, cx);
            field.move_to(typed, cx);
            field
        });
        query.focus_handle(cx).tab_stop(true);
        cx.subscribe(&query, |this, input, _: &TextChanged, cx| {
            let text = input.read(cx).as_str().to_owned();
            if let Some(clip) = this.clip.as_mut() {
                clip.set_query(&text);
                cx.notify();
                return;
            }
            if text != this.state.query {
                this.state.set_query(&text);
                this.selection_moved(cx);
            }
        })
        .detach();
        let filter = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
        cx.subscribe(&filter, |this, input, _: &TextChanged, cx| {
            let text = input.read(cx).as_str().to_owned();
            if let Some(panel) = this.state.actions.as_mut()
                && panel.query != text
            {
                panel.query = text;
                panel.selected = 0;
                cx.notify();
            }
        })
        .detach();
        FixtureWindow {
            scenario,
            perturbation,
            state,
            query,
            scroll: ScrollHandle::new(),
            pointer: None,
            filter,
            clip: (scenario.family == Family::Clipboard).then(|| ClipState::new(scenario)),
            board_material: 0,
            select: (pages::pane_page(scenario.name) == Some(pages::PanePage::Launcher))
                .then(|| pages::monitor_select(window, cx)),
        }
    }

    /// The footer's Actions button: opens the panel with focus in its
    /// search, or closes it and gives focus back to the query, as the
    /// launcher's does.
    fn toggle_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.toggle_actions();
        let focus = if self.state.actions.is_some() {
            self.filter.update(cx, |filter, cx| filter.emplace("", cx));
            self.filter.focus_handle(cx)
        } else {
            self.query.focus_handle(cx)
        };
        window.focus(&focus, cx);
        cx.notify();
    }

    fn close_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.actions.is_some() {
            self.toggle_actions(window, cx);
        }
    }

    fn actions_next(
        &mut self,
        _: &actions_panel::NextAction,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.down();
        cx.notify();
    }

    fn actions_previous(
        &mut self,
        _: &actions_panel::PreviousAction,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let listed = self.state.listed_actions();
        if let Some(panel) = self.state.actions.as_mut()
            && let Some(previous) = actions_panel::next_available(&listed, panel.selected, false)
        {
            panel.selected = previous;
            cx.notify();
        }
    }

    fn actions_close(
        &mut self,
        _: &actions_panel::CloseActions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_actions(window, cx);
    }

    /// The pointer moved over row `index` to `position`: real movement
    /// selects it, as the launcher's root search does, without scrolling
    /// the list (only the keys' selection scrolls).
    fn pointer_moved_over(
        &mut self,
        index: usize,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let moved = self.pointer.is_some_and(|last| last != position);
        // The open panel holds its target, as the launcher's does.
        if moved && self.state.selected != index && self.state.actions.is_none() {
            self.state.pointer_over(index);
            cx.notify();
        }
    }

    /// After the selection may have moved: the list keeps the selected
    /// row in view, as the launcher's does, and the window redraws.
    fn selection_moved(&mut self, cx: &mut Context<Self>) {
        self.scroll.scroll_to_item(self.state.selected_child());
        cx.notify();
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.state.select_next();
        self.selection_moved(cx);
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.state.select_previous();
        self.selection_moved(cx);
    }

    /// The launcher's Back, under its effective binding: the query clears
    /// and the selection returns to the first row.
    fn back(&mut self, _: &Back, _: &mut Window, cx: &mut Context<Self>) {
        self.state.back();
        let query = self.query.clone();
        query.update(cx, |query, cx| query.emplace("", cx));
        self.selection_moved(cx);
    }

    /// The theme this frame renders with: the settings' theme, with the
    /// perturbation applied when the run injects one.
    fn theme(&self, cx: &App) -> Theme {
        let mut theme = settings::visuals(cx).theme;
        if let Some(perturbation) = self.perturbation {
            perturbation.apply(&mut theme);
        }
        theme
    }

    fn render_keycaps(&self, theme: &Theme) -> gpui::Div {
        let caps = KEY_GROUPS.iter().map(|group| {
            let keys = crate::keyboard::binding_keys(&parse_binding(group.binding));
            // Each group gets its own scope: the group's id is fixed.
            div()
                .id(SharedString::from(format!("keys-{}", group.binding)))
                .flex()
                .child(keycap::key_sequence(&keys, group.style, theme))
        });
        div()
            .key_context(KEY_CONTEXT)
            .size_full()
            .flex()
            .flex_col()
            .items_start()
            .p(px(KEYCAP_INSET))
            .gap(px(KEYCAP_GAP))
            .font_family(theme.typography.family.clone())
            .font_features(theme.typography.features.clone())
            .text_color(theme.text_title)
            .children(caps)
    }

    fn render_tiles(&self, theme: &Theme) -> gpui::Div {
        div()
            .size_full()
            .flex()
            .items_start()
            .p(px(KEYCAP_INSET))
            .gap(px(TILE_GAP))
            .children(
                TILES
                    .iter()
                    .map(|&(size, tone, glyph)| tile_at(size, tone, glyph, theme)),
            )
    }

    /// The Settings board's shell over the board's own data, through the
    /// Settings window's own composition (`features::settings::compose`):
    /// the titlebar, the sidebar with its search field and section items,
    /// and the page (`ui::settings_shell`), on the Settings panel. The
    /// shell's scenarios show the board's heading and its preview column's
    /// caption only; the Appearance scenarios show the page itself (#98:
    /// see [`appearance_page`]).
    fn render_settings(
        &self,
        theme: &Theme,
        material: Material,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let pane_page = pages::pane_page(self.scenario.name);
        let selected = pages::selected_section(pane_page, SETTINGS_SELECTED);
        let sections = SETTINGS_SECTIONS
            .iter()
            .enumerate()
            .map(|(index, section)| {
                settings_shell::sidebar_item(
                    ("section", index),
                    SidebarItem {
                        label: section.label.into(),
                        glyph: section.glyph,
                        detail: None,
                        reason: None,
                        count: section.count.map(Into::into),
                        selected: index == selected,
                    },
                    theme,
                )
            });
        let search =
            settings_shell::search_field(&self.query, settings_shell::SEARCH_PLACEHOLDER, theme);
        let sections = settings_shell::section_list(theme).children(sections);
        let sidebar = settings_shell::sidebar(search, sections, theme);
        let page = settings_shell::page_viewport(theme);
        let page = match appearance_page(self.scenario.name) {
            // Pane's own pages, through their own compositions (#99).
            None if pane_page.is_some() => page.children(
                pane_page.map(|pane| pages::render_page(pane, self.select.as_ref(), theme, cx)),
            ),
            Some(AppearancePage::Board) => page.child(self.render_appearance_board(theme, cx)),
            Some(AppearancePage::Production { overridden }) => {
                page.child(render_appearance_production(overridden, theme, cx))
            }
            None => {
                let header = settings_shell::page_header(
                    SETTINGS_HEADING,
                    Some(SETTINGS_SUBTITLE.into()),
                    theme,
                );
                let aside = settings_shell::aside(SETTINGS_ASIDE, theme);
                page.child(settings_shell::page_columns(header, aside, theme))
            }
        };
        crate::features::settings::compose(div(), "Settings", sidebar, page, theme, material)
    }

    /// The Appearance board's page (#98): its fixture content — the
    /// Material choice (the board's Glass, Frost and Solid), its accent
    /// swatches, the blur and tint sliders, Density and the two toggles —
    /// through the production control families (`ui::controls`), beside
    /// the preview's stage and miniature (`ui::preview`) and the board's
    /// store link. A click on a material segment chooses it, as the
    /// board's does: Solid dims the sliders to the board's 40% (keeping
    /// their values) and puts the miniature on the solid surface. The
    /// controls past the Material choice are the board's deferred ones
    /// (#100): they do nothing here either.
    fn render_appearance_board(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let chosen = self.board_material;
        let solid = chosen == BOARD_SOLID;
        let dimmed = |field: gpui::Div| {
            field.when(solid, |field| {
                field.opacity(theme.geometry.controls.disabled_opacity)
            })
        };
        let materials: Vec<_> = BOARD_MATERIALS
            .iter()
            .enumerate()
            .map(|(index, &(name, _))| {
                controls::segment(name, index == chosen, true, theme)
                    .id(SharedString::from(segment_target(name)))
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.board_material = index;
                        cx.notify();
                    }))
            })
            .collect();
        let material = controls::field(theme)
            .child(controls::field_label("Material", theme))
            .child(controls::segment_track(theme).children(materials))
            .child(controls::field_description(
                BOARD_MATERIALS[chosen].1,
                theme.text_muted,
                theme,
            ));
        let swatches = BOARD_SWATCHES
            .iter()
            .enumerate()
            .map(|(index, &(_, fill))| {
                controls::swatch(gpui::rgb_to_hsla(gpui::rgba(fill)), index == 0, theme)
            });
        let accent = controls::field(theme)
            .child(controls::field_label("Accent", theme))
            .child(
                controls::swatch_row(theme)
                    .children(swatches)
                    .child(controls::swatch_add(theme)),
            );
        let slider = |(label, value, fraction): (&str, &str, f32)| {
            dimmed(controls::field(theme))
                .child(controls::slider_header(label, value, theme))
                .child(controls::slider(fraction, theme))
        };
        let blur = slider(BOARD_BLUR);
        let tint = slider(BOARD_TINT).child(controls::field_description(
            BOARD_TINT_NOTE,
            theme.text_muted,
            theme,
        ));
        let densities = BOARD_DENSITIES.iter().enumerate().map(|(index, &name)| {
            controls::segment(name, index == BOARD_DENSITY, true, theme)
                .id(SharedString::from(segment_target(name)))
        });
        let density = controls::field(theme)
            .child(controls::field_label("Density", theme))
            .child(controls::segment_track(theme).children(densities));
        let toggles = controls::toggle_list().children(
            BOARD_TOGGLES
                .iter()
                .map(|&label| controls::toggle_row(label, controls::toggle(true, theme), theme)),
        );
        let column = controls::column(theme)
            .child(settings_shell::page_header(
                SETTINGS_HEADING,
                Some(SETTINGS_SUBTITLE.into()),
                theme,
            ))
            .child(material)
            .child(accent)
            .child(blur)
            .child(tint)
            .child(density)
            .child(toggles);
        // The board's Glass and Frost are glass; its Solid, the solid
        // surface.
        let surface = Material::new(if solid {
            MaterialMode::Opaque
        } else {
            MaterialMode::Glass
        });
        let content = board_preview(&settings::keyboard_of(cx));
        let stage = preview::stage(preview::miniature(&content, surface, theme), theme);
        let aside = settings_shell::aside(SETTINGS_ASIDE, theme)
            .child(stage)
            .child(preview::link(BOARD_LINK, theme));
        div()
            .id("appearance")
            .child(settings_shell::page_columns(column, aside, theme))
    }

    /// A result board's extension suggestions under their label (#96),
    /// through the shared result layouts: fixture content only.
    fn render_suggestions(&self, theme: &Theme) -> Vec<gpui::AnyElement> {
        let Some(suggestions) = self.state.suggestions() else {
            return Vec::new();
        };
        let keys = crate::keyboard::binding_keys(&parse_binding(suggestions.keys));
        let label = result_layouts::keyed_section_label(
            suggestions.label.into(),
            suggestions.note.into(),
            &keys,
            theme,
        );
        let rows = suggestions.items.iter().enumerate().map(|(index, item)| {
            result_layouts::suggestion_row(("suggestion", index), &item.suggestion(), theme)
                .into_any_element()
        });
        std::iter::once(label.into_any_element())
            .chain(rows)
            .collect()
    }

    fn render_root(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let rows = self.state.rows.iter().enumerate().map(|(index, row)| {
            let selected = index == self.state.selected;
            // A result board's card and history rows (#96), through the
            // shared result layouts.
            if let Some(answer) = self.state.answer_at(index) {
                let press = crate::ui::theme::pressed(theme.results.card_fill);
                return answer
                    .draw(selected, theme)
                    .id(("row", index))
                    .active(move |card| card.bg(press))
                    .role(Role::ListBoxOption)
                    .aria_label(answer.label(row.title))
                    .aria_selected(selected)
                    .into_any_element();
            }
            if self.state.history_at(index) {
                let history = HistoryRow {
                    expression: row.title.into(),
                    answer: row.subtitle.unwrap_or_default().into(),
                    kind: row.kind.into(),
                    icon: row.icon,
                    selected,
                };
                let press = crate::ui::result_row::pressed_wash(selected, theme);
                return result_layouts::history_row(&history, theme)
                    .id(("row", index))
                    .active(move |row| row.bg(press))
                    .role(Role::ListBoxOption)
                    .aria_label(row.title)
                    .aria_selected(selected)
                    .into_any_element();
            }
            result_row_with(
                RowContent {
                    title: row.title.into(),
                    subtitle: row.subtitle.map(Into::into),
                    unavailable_reason: row.unavailable.map(Into::into),
                    unavailable_id: ("unavailable", index).into(),
                    selected,
                    icon: Some(row.icon),
                },
                RowMeta {
                    matched: pane_core::title_matches(row.title, &self.state.query),
                    alias: row.alias.map(Into::into),
                    keys: row
                        .keys
                        .map(|binding| crate::keyboard::binding_keys(&parse_binding(binding))),
                    kind: Some(row.kind.into()),
                    number: None,
                },
                theme,
            )
            .id(("row", index))
            .active({
                let press = crate::ui::result_row::pressed_wash(selected, theme);
                move |row| row.bg(press)
            })
            .role(Role::ListBoxOption)
            .aria_label(row.title)
            .aria_selected(selected)
            .on_mouse_move(
                cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                    this.pointer_moved_over(index, event.position, cx);
                }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                // A click selects the row it lands on; the fixture opens
                // nothing — there is no command behind fixture data.
                this.state.selected = index;
                cx.notify();
            }))
            .into_any_element()
        });
        // The pinned home over a blank query, through the launcher's own
        // composition (`ui::pinned`): the fixture's authored pins, then the
        // pin hint while its cell is in the strip's last row.
        let home = self.state.home_shown().then(|| {
            let pins = self.state.pins;
            let mut tiles: Vec<gpui::AnyElement> = pins
                .iter()
                .enumerate()
                .map(|(index, pin)| {
                    let content = SlotContent {
                        index,
                        title: pin.title.into(),
                        icon: pin.icon,
                        number: None,
                        unavailable: pin.unavailable.map(Into::into),
                    };
                    pinned::pinned_slot(content, theme).into_any_element()
                })
                .collect();
            if pinned::shows_pin_hint(pins.len()) {
                tiles.push(pinned::pin_hint(theme).into_any_element());
            }
            pinned::home(tiles, theme)
        });
        // A result board's notice for its query (#96), as root search
        // composes it.
        let notice = self
            .state
            .notice()
            .map(|copy| root_search::layouts::notice(&copy, theme).into_any_element());
        // The launcher's own result list, after the notice when it shows,
        // then a board's suggestions.
        let list = shell::result_list(theme)
            .aria_label("Results")
            .track_scroll(&self.scroll)
            // The pinned home over a blank query, above the rest (#101).
            .when_some(home, |list, home| list.children(home))
            .children(root_search::layouts::list_children(
                notice,
                rows,
                &self.state.sections(),
                theme,
            ))
            .children(self.render_suggestions(theme));
        let focus = self.query.focus_handle(cx);
        let search = div()
            .id("search")
            .key_context(root_search::search_context())
            .track_focus(&focus)
            .role(Role::EditableComboBox)
            .aria_label("Search")
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(search_header(
                &self.query,
                root_search::ROOT_PLACEHOLDER,
                theme,
            ))
            // The results, under the dimmer while the panel is open, as
            // the launcher composes them.
            .child(actions_panel::dimmed(
                list.into_any_element(),
                self.state.actions.is_some(),
                theme,
            ));
        // The footer, composed as the launcher's (`ui::footer`): the mark
        // (the launcher's app menu; the fixture opens no menu), the hint
        // and the buttons — the selected row's primary action, the rule
        // and Actions — and the open panel over the strip.
        let keyboard = settings::keyboard_of(cx);
        let invoke = keyboard
            .binding(KeyboardAction::InvokeSelectedAction)
            .clone();
        let invoke_keys = crate::keyboard::binding_keys(&invoke);
        let open_keys =
            crate::keyboard::binding_keys(keyboard.binding(KeyboardAction::OpenActions));
        // The selected row's primary action, as the reference's footer
        // follows the selection; no row, no button.
        let action = self
            .state
            .rows
            .get(self.state.selected)
            .map(|row| SelectedAction {
                label: row.action.into(),
                available: true,
            });
        let open = self.state.actions.is_some();
        let buttons = footer::buttons(
            action.map(|action| action_button(&action, &invoke, theme).into_any_element()),
            Some(
                footer::actions_button(&open_keys, open, theme)
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_actions(window, cx)))
                    .into_any_element(),
            ),
            theme,
        );
        let hint = footer::hint_line(
            footer::hint_parts(
                open,
                invoke_keys.clone(),
                open_keys,
                crate::keyboard::escape_keys(),
            ),
            theme,
        );
        let panel = self.state.actions.as_ref().map(|panel| {
            let target = self.state.target_actions();
            let kind = self
                .state
                .rows
                .get(self.state.selected)
                .and_then(|row| row_kind(row.kind));
            let listed = self.state.listed_actions();
            let surface = actions_panel::compose(
                actions_panel::PanelView {
                    target: target.as_ref().map(|actions| (actions, kind)),
                    icon: self.state.rows.get(self.state.selected).map(|row| row.icon),
                    listed: &listed,
                    group: actions_panel::PANE_GROUP,
                    filtering: !panel.query.trim().is_empty(),
                    selected: panel.selected,
                    invoke: &invoke_keys,
                    // The reference's panel shows no keys on its pin entry.
                    slot_keys: None,
                    filter: &self.filter,
                },
                theme,
                settings::visuals(cx).material,
                |row, _| row,
            )
            .key_context(actions_panel::CONTEXT)
            .on_action(cx.listener(Self::actions_next))
            .on_action(cx.listener(Self::actions_previous))
            .on_action(cx.listener(Self::actions_close))
            .on_mouse_down_out(cx.listener(
                |this, _: &gpui::MouseDownEvent, window, cx| {
                    this.close_actions(window, cx);
                    cx.stop_propagation();
                },
            ));
            actions_panel::anchored(surface, theme)
        });
        let footer = Material::footer(theme)
            .id("status")
            .relative()
            .when_some(panel, |strip, panel| strip.child(panel))
            .text_size(theme.typography.footer_size)
            .child(footer::footer_row(
                footer::mark_button(theme).into_any_element(),
                footer::hint_slot(Some(hint), theme).into_any_element(),
                buttons,
                theme,
            ));
        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::back))
            // After the rows' own handlers: a row compares the event with
            // the position before it.
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, _| {
                this.pointer = Some(event.position);
            }))
            .size_full()
            .flex()
            .flex_col()
            .font_family(theme.typography.family.clone())
            .font_features(theme.typography.features.clone())
            .text_color(theme.text_title)
            .child(search)
            .child(footer)
    }
}

impl Focusable for FixtureWindow {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.query.focus_handle(cx)
    }
}

impl Render for FixtureWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let material = settings::visuals(cx).material;
        let theme = self.theme(cx);
        let content = match self.scenario.family {
            Family::Keycap => self.render_keycaps(&theme),
            Family::Tiles => self.render_tiles(&theme),
            Family::Root => self.render_root(&theme, cx),
            Family::Clipboard => self.render_clipboard(&theme, cx),
            Family::Form => pages::render_form(self, &theme, cx),
            // The Settings window's composition brings its own panel.
            Family::Settings => return self.render_settings(&theme, material, cx),
        };
        material.panel(&theme, content)
    }
}

/// The fixture's manifest: the declared values the comparison measures
/// against, from what the fixture actually rendered with — the theme the
/// settings resolved, the layout the composition lays out, the bindings
/// in force, the label widths the text system shaped — in logical client
/// coordinates, with the window's own bounds and scale.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    kind: &'static str,
    scenario: &'static Scenario,
    perturbation: Option<&'static str>,
    /// The panel material and theme in force (the comparison reads
    /// colors strictly only in the opaque material).
    material: &'static str,
    appearance: &'static str,
    window: WindowRecord,
    declared: DeclaredTokens,
    search_header: Rect,
    list: Rect,
    footer: Rect,
    captures: Vec<DeclaredCapture>,
    /// The scenario's steps, with each pointer step's client point.
    steps: Vec<ResolvedStep>,
    keycaps: Vec<KeyGroupRecord>,
    tiles: Vec<TileRecord>,
    /// The Settings shell, for the Settings family's scenarios.
    #[serde(skip_serializing_if = "Option::is_none")]
    settings: Option<DeclaredSettings>,
    /// The Appearance page, capture by capture, for its scenarios (#98).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    appearance_page: Vec<DeclaredAppearance>,
    /// Pane's own page or the launcher's form, capture by capture, for
    /// their scenarios (#99).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pane_page: Vec<pages::DeclaredPage>,
    fonts: Vec<FontRecord>,
    /// How the text system resolved each embedded face (see
    /// [`FontResolution`]).
    font_resolution: Vec<FontResolution>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowRecord {
    /// [`Window::bounds`]: origin and logical size, as GPUI reports them.
    bounds: [f32; 4],
    /// [`Window::viewport_size`]: the logical client the fixture paints.
    viewport: [f32; 2],
    scale_factor: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclaredTokens {
    geometry: GeometryRecord,
    colors: ColorRecord,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeometryRecord {
    search_height: f32,
    search_padding_x: f32,
    search_gap: f32,
    search_size: f32,
    row_min_height: f32,
    row_radius: f32,
    row_padding_x: f32,
    row_gap: f32,
    row_list_gap: f32,
    row_title_size: f32,
    row_subtitle_size: f32,
    list_padding_top: f32,
    list_padding_bottom: f32,
    list_padding_x: f32,
    tile_size: f32,
    tile_radius: f32,
    footer_height: f32,
    footer_padding_left: f32,
    footer_padding_right: f32,
    action_height: f32,
    action_radius: f32,
    action_padding_x: f32,
    action_gap: f32,
    keycap_height: f32,
    keycap_radius: f32,
    keycap_padding_x: f32,
    keycap_compact_height: f32,
    keycap_compact_padding_x: f32,
    key_gap: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ColorRecord {
    panel_solid: Hex,
    keycap_background: Hex,
    keycap_bottom: Hex,
    alias_text: Hex,
    keycap_text: Hex,
    accent: Hex,
    accent_ink: Hex,
    panel_top_highlight: Hex,
    row_hover: Hex,
    row_selected: Hex,
    row_selected_border: Hex,
    footer_tint: Hex,
    hairline: Hex,
    hairline_soft: Hex,
    tile_background: Hex,
    tile_foreground: Hex,
    text_title: Hex,
    text_muted: Hex,
    text_query: Hex,
    text_placeholder: Hex,
    popover_solid: Hex,
    popover_edge: Hex,
    footer_mark: Hex,
    footer_button_text: Hex,
    control_hover: Hex,
    footer_button_open: Hex,
    footer_divider: Hex,
    action_selected: Hex,
    action_text: Hex,
    action_icon: Hex,
    action_rule: Hex,
    actions_dimmer: Hex,
    slot_background: Hex,
    slot_edge: Hex,
    slot_hover: Hex,
    slot_title: Hex,
}

/// A color as the manifest writes it: `#RRGGBBAA`.
struct Hex(Hsla);

impl Serialize for Hex {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex(self.0))
    }
}

fn hex(color: Hsla) -> String {
    let rgba = gpui::hsla_to_rgba(color);
    let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        channel(rgba.color.red),
        channel(rgba.color.green),
        channel(rgba.color.blue),
        channel(rgba.alpha)
    )
}

/// One key sequence as the manifest declares it: its caps, where each
/// lies, and the type they are set in.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct KeyGroupRecord {
    /// The binding the sequence shows, in the record's grammar.
    binding: &'static str,
    /// The reference key group it pairs with, if any.
    group: Option<&'static str>,
    style: &'static str,
    /// The sequence's accessible name.
    name: String,
    /// The caps' type: "11px Geist Mono 500".
    font: String,
    rect: Rect,
    caps: Vec<CapRecord>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CapRecord {
    /// The cap's label, as both sides are controlled to show it.
    label: String,
    /// The label's shaped width at the cap's font, size and weight.
    label_width: f32,
    rect: Rect,
}

/// One tile of the tile scenario.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TileRecord {
    size: &'static str,
    /// "app" (a gradient tone, 2px glyph) or "command" (the neutral tile).
    tone: &'static str,
    rect: Rect,
    glyph: f32,
}

/// How the text system resolved one of the embedded faces: a face the
/// text system could not find resolves to its fallback — the same face as
/// the probe for a family no system has — so a missing Geist is visible
/// in the manifest rather than silently drawn in another font.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FontResolution {
    family: SharedString,
    weight: f32,
    font_id: usize,
    /// The shaped width of [`FONT_PROBE`] at 14px in the face.
    probe_width: f32,
}

/// The text the font resolution check shapes in every face.
const FONT_PROBE: &str = "Geist Ctrl+Shift+V";

/// A family no system has: it resolves to the text system's fallback.
const MISSING_FONT: &str = "Pane Missing Font Probe";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FontRecord {
    file: &'static str,
    bytes: usize,
    sha256: String,
}

/// The width `text` shapes to in `theme`'s family at `size` and `weight`.
fn shaped_width(
    window: &Window,
    theme: &Theme,
    text: &str,
    size: Pixels,
    weight: FontWeight,
) -> f32 {
    shaped_width_in(
        window,
        theme.typography.family.clone(),
        theme,
        text,
        size,
        weight,
    )
}

/// The width `text` shapes to in `family` at `size` and `weight`.
fn shaped_width_in(
    window: &Window,
    family: SharedString,
    theme: &Theme,
    text: &str,
    size: Pixels,
    weight: FontWeight,
) -> f32 {
    shaped_width_tracked(window, family, theme, text, size, weight, None)
}

/// Where `row`'s trailing parts lie, right-aligned inside its padding
/// with the row's gap between them, as the shared row lays them out: the
/// kind (at least its least width, its text right-aligned), the key
/// sequence, then the alias chip.
fn declare_trailing(window: &Window, theme: &Theme, row: &mut DeclaredRow) {
    let geometry = &theme.geometry;
    let typography = &theme.typography;
    let gap = f32::from(geometry.row_gap);
    let center = row.rect.y + row.rect.height / 2.;
    let mut right = row.rect.x + row.rect.width - f32::from(geometry.row_padding_x);
    let kind_width = shaped_width(
        window,
        theme,
        row.kind,
        typography.row_kind_size,
        typography.regular,
    )
    .max(f32::from(geometry.row_kind_min_width));
    row.kind_rect = Some(Rect {
        x: right - kind_width,
        y: row.rect.y,
        width: kind_width,
        height: row.rect.height,
    });
    right -= kind_width + gap;
    if let Some(binding) = row.keys {
        let height = f32::from(geometry.keycap_height);
        let mut group = declared_group(
            window,
            theme,
            (binding, &parse_binding(binding)),
            None,
            CapStyle::Regular,
            (0., center - height / 2.),
        );
        let shift = right - group.rect.width;
        group.rect.x += shift;
        for cap in &mut group.caps {
            cap.rect.x += shift;
        }
        right -= group.rect.width + gap;
        row.key_group = Some(group);
    }
    if let Some(alias) = row.alias {
        let width = shaped_width_in(
            window,
            typography.mono_family.clone(),
            theme,
            alias,
            typography.alias_size,
            FontWeight::NORMAL,
        ) + 2. * f32::from(geometry.alias_padding_x);
        row.alias_rect = Some(Rect {
            x: right - width,
            y: row.rect.y,
            width,
            height: row.rect.height,
        });
    }
}

/// The width `text` shapes to in `family` at `size` and `weight`, with
/// `spacing` added after each character where it has some, as a tracked
/// run is laid out (the answer card's values, #96).
fn shaped_width_tracked(
    window: &Window,
    family: SharedString,
    theme: &Theme,
    text: &str,
    size: Pixels,
    weight: FontWeight,
    spacing: Option<Pixels>,
) -> f32 {
    let mut font = gpui::font(family);
    font.weight = weight;
    // The features every text run asks for (kerning), as rendered.
    font.features = theme.typography.features.clone();
    let run = TextRun {
        len: text.len(),
        font,
        color: theme.text_title,
        background_color: None,
        underline: None,
        strikethrough: None,
        letter_spacing: spacing,
    };
    window
        .text_system()
        .shape_line(SharedString::from(text.to_owned()), size, &[run], None)
        .width()
        .into()
}

/// A value column's height on the answer card: the value's line box, over
/// its caption's 4 below it where it has one (#96).
fn answer_column_height(side: &AnswerSide, value: TypeLine, theme: &Theme) -> f32 {
    let f = f32::from;
    let caption = side.caption.as_ref().map_or(0., |_| {
        f(theme.geometry.results.card_value_gap)
            + f(theme.typography.results.answer_caption.line_height)
    });
    f(value.line_height) + caption
}

/// The card's values line's height: its taller column, or the arrow's
/// disc where that is taller (#96).
fn answer_values_height(card: &AnswerCard, theme: &Theme) -> f32 {
    let value = result_layouts::answer_value_type(card, theme);
    answer_column_height(&card.source, value, theme)
        .max(answer_column_height(&card.answer, value, theme))
        .max(f32::from(theme.geometry.results.card_arrow_disc))
}

/// The answer card's height as the shared layout lays `card` out (#96):
/// its paddings around its values line and, where it has chips, the gap,
/// the 1px rule and the padding above the chips' line.
fn answer_card_height(card: &AnswerCard, theme: &Theme) -> f32 {
    let results = &theme.geometry.results;
    let f = f32::from;
    let values = answer_values_height(card, theme);
    let also = if card.also.is_empty() {
        0.
    } else {
        f(results.card_gap) + 1. + f(results.also_padding_top) + f(results.chip_height)
    };
    f(results.card_padding_top) + values + also + f(results.card_padding_bottom)
}

/// Where a result board's own parts lie in `capture` (#96), as the shared
/// result layouts lay them out: the notice heading the list, the answer
/// card in its row's place, and the suggestions after the rows, under
/// their label, which joins the capture's sections.
fn declare_board(
    window: &Window,
    theme: &Theme,
    frame: &Frame,
    board: &ResultBoard,
    capture: &mut DeclaredCapture,
) {
    let geometry = &theme.geometry;
    let f = f32::from;
    let x = frame.list.x + f(geometry.list_padding_x);
    let width = frame.list.width - 2. * f(geometry.list_padding_x);
    let top = frame.list.y + f(geometry.list_padding_top);
    let notice = board.notice.then(|| {
        let copy = root_search::layouts::notice_copy(board.query, !capture.rows.is_empty());
        let rect = Rect {
            x,
            y: top,
            width,
            height: f(geometry.results.notice_height),
        };
        declared_notice(theme, &copy, rect)
    });
    let answer = board.answer.as_ref().and_then(|answer| {
        let row = capture.rows.iter().find(|row| row.answer)?;
        Some(declared_answer(window, theme, answer, row))
    });
    let mut suggestions = Vec::new();
    if let Some(items) = &board.suggestions {
        // After the last row and the list's gap, or the notice's.
        let gap = f(geometry.row_list_gap);
        let after = match (capture.rows.last(), &notice) {
            (Some(row), _) => row.rect.y + row.rect.height + gap,
            (None, Some(notice)) => notice.rect.y + notice.rect.height + gap,
            (None, None) => top,
        };
        let (label, rows) = declared_suggestions(window, theme, items, (x, after, width));
        capture.sections.push(label);
        suggestions = rows;
    }
    capture.board = Some(DeclaredBoard {
        notice,
        answer,
        suggestions,
    });
}

/// The notice in `rect`, saying `copy`: its disc 12 in and centered in the
/// notice's padded height, its text 16 after the disc, the title's line
/// over the description's, 4 apart, centered beside it (#96).
fn declared_notice(theme: &Theme, copy: &NoticeCopy, rect: Rect) -> DeclaredNotice {
    let results = &theme.geometry.results;
    let types = &theme.typography.results;
    let f = f32::from;
    let content_top = rect.y + f(results.notice_padding_top);
    let content = rect.height - f(results.notice_padding_top) - f(results.notice_padding_bottom);
    let side = f(results.notice_disc);
    let disc = Rect {
        x: rect.x + f(results.notice_padding_x),
        y: content_top + (content - side) / 2.,
        width: side,
        height: side,
    };
    let (title_line, description_line) = (
        f(types.notice_title.line_height),
        f(types.notice_description.line_height),
    );
    let text_height = title_line + f(results.notice_text_gap) + description_line;
    let text_x = disc.x + side + f(results.notice_gap);
    let text_top = content_top + (content - text_height) / 2.;
    let text_width = rect.x + rect.width - f(results.notice_padding_x) - text_x;
    let line = |text: &SharedString, y: f32, height: f32| DeclaredText {
        text: text.to_string(),
        rect: Rect {
            x: text_x,
            y,
            width: text_width,
            height,
        },
    };
    let description_top = text_top + title_line + f(results.notice_text_gap);
    DeclaredNotice {
        rect,
        disc,
        disc_fill: hex(theme.results.notice_disc),
        title: line(&copy.title, text_top, title_line),
        description: line(&copy.description, description_top, description_line),
    }
}

/// The answer card in `row`'s place (#96): its values centered in their
/// columns either side of the arrow's disc, each over its caption, the
/// values line centered in its height; then, where it has chips, the rule
/// and the "Also" line, its label and chips centered across the card.
fn declared_answer(
    window: &Window,
    theme: &Theme,
    answer: &FixtureAnswer,
    row: &DeclaredRow,
) -> DeclaredAnswer {
    let results = &theme.geometry.results;
    let types = &theme.typography.results;
    let colors = &theme.results;
    let f = f32::from;
    let card = answer.card(row.selected);
    let value = result_layouts::answer_value_type(&card, theme);
    let rect = row.rect;
    let inner_x = rect.x + f(results.card_padding_x);
    let inner_width = rect.width - 2. * f(results.card_padding_x);
    let (arrow, gap) = (f(results.card_arrow_disc), f(results.card_column_gap));
    let column = (inner_width - arrow - 2. * gap) / 2.;
    let values_height = answer_values_height(&card, theme);
    let values_top = rect.y + f(results.card_padding_top);
    let mono = theme.typography.mono_family.clone();
    let spacing = value.size * types.answer_tracking;
    let side = |part: &AnswerSide, left: f32, color: Hsla| {
        let top = values_top + (values_height - answer_column_height(part, value, theme)) / 2.;
        let width = shaped_width_tracked(
            window,
            mono.clone(),
            theme,
            &part.value,
            value.size,
            theme.typography.medium,
            Some(spacing),
        );
        let caption = part.caption.as_ref().map(|caption| {
            let size = types.answer_caption.size;
            let width = shaped_width(window, theme, caption, size, theme.typography.regular);
            DeclaredText {
                text: caption.to_string(),
                rect: Rect {
                    x: left + (column - width) / 2.,
                    y: top + f(value.line_height) + f(results.card_value_gap),
                    width,
                    height: f(types.answer_caption.line_height),
                },
            }
        });
        DeclaredSide {
            value: DeclaredText {
                text: part.value.to_string(),
                rect: Rect {
                    x: left + (column - width) / 2.,
                    y: top,
                    width,
                    height: f(value.line_height),
                },
            },
            color: hex(color),
            caption,
        }
    };
    let source = side(&card.source, inner_x, colors.card_source);
    let answered = side(
        &card.answer,
        inner_x + column + gap + arrow + gap,
        colors.card_answer,
    );
    let arrow_rect = Rect {
        x: inner_x + column + gap,
        y: values_top + (values_height - arrow) / 2.,
        width: arrow,
        height: arrow,
    };
    let (mut also, mut chips, mut rule) = (None, Vec::new(), None);
    if !card.also.is_empty() {
        let rule_y = values_top + values_height + f(results.card_gap);
        let line_top = rule_y + 1. + f(results.also_padding_top);
        let chip_height = f(results.chip_height);
        let label_line = f(types.answer_also.line_height);
        let label_width = shaped_width(
            window,
            theme,
            "Also",
            types.answer_also.size,
            theme.typography.regular,
        );
        let widths: Vec<f32> = card
            .also
            .iter()
            .map(|chip| {
                let regular = theme.typography.regular;
                shaped_width_in(window, mono.clone(), theme, chip, types.chip.size, regular)
                    + 2. * f(results.chip_padding_x)
            })
            .collect();
        let also_gap = f(results.also_gap);
        let total = label_width + widths.iter().map(|width| also_gap + width).sum::<f32>();
        let mut left = inner_x + (inner_width - total) / 2.;
        also = Some(DeclaredText {
            text: "Also".into(),
            rect: Rect {
                x: left,
                y: line_top + (chip_height - label_line) / 2.,
                width: label_width,
                height: label_line,
            },
        });
        left += label_width;
        for (chip, width) in card.also.iter().zip(widths) {
            left += also_gap;
            chips.push(DeclaredText {
                text: chip.to_string(),
                rect: Rect {
                    x: left,
                    y: line_top,
                    width,
                    height: chip_height,
                },
            });
            left += width;
        }
        rule = Some(Rect {
            x: inner_x,
            y: rule_y,
            width: inner_width,
            height: 1.,
        });
    }
    DeclaredAnswer {
        rect,
        selected: card.selected,
        fill: hex(colors.card_fill),
        ring: hex(theme.accent_text),
        font: format!(
            "{}px {} {}",
            f(value.size),
            theme.typography.mono_family,
            theme.typography.medium.0
        ),
        source,
        answer: answered,
        arrow: arrow_rect,
        arrow_fill: hex(colors.card_arrow_disc),
        also,
        chips,
        chip_fill: hex(colors.chip_fill),
        rule,
    }
}

/// The suggestions' label at `(x, y)` across `width`, then each
/// suggestion's row, 54 high, the list's gap apart: its tile 10 in and
/// centered, its pill at the row's right padding, centered (#96).
fn declared_suggestions(
    window: &Window,
    theme: &Theme,
    suggestions: &FixtureSuggestions,
    (x, y, width): (f32, f32, f32),
) -> (DeclaredSection, Vec<DeclaredSuggestion>) {
    let geometry = &theme.geometry;
    let results = &geometry.results;
    let f = f32::from;
    let gap = f(geometry.row_list_gap);
    let keys = crate::keyboard::binding_keys(&parse_binding(suggestions.keys));
    let caps: String = keys.keys.iter().map(|key| key.cap.as_ref()).collect();
    let label = DeclaredSection {
        label: suggestions.label.to_owned(),
        // As the reference's DOM reads the note: its text, then its caps.
        note: Some(format!("{}{caps}", suggestions.note)),
        rect: Rect {
            x,
            y,
            width,
            height: f(geometry.section_height),
        },
    };
    let mut top = y + f(geometry.section_height) + gap;
    let height = f(results.suggestion_height);
    let tile = f(results.suggestion_tile.size);
    let pill_height = f(results.pill_height);
    let rows = suggestions
        .items
        .iter()
        .map(|item| {
            let size = theme.typography.results.pill.size;
            let label = shaped_width(window, theme, item.action, size, theme.typography.medium);
            let pill = label + 2. * f(results.pill_padding_x);
            let row = DeclaredSuggestion {
                title: item.title.to_owned(),
                rect: Rect {
                    x,
                    y: top,
                    width,
                    height,
                },
                tile: Rect {
                    x: x + f(geometry.row_padding_x),
                    y: top + (height - tile) / 2.,
                    width: tile,
                    height: tile,
                },
                pill: Rect {
                    x: x + width - f(geometry.row_padding_x) - pill,
                    y: top + (height - pill_height) / 2.,
                    width: pill,
                    height: pill_height,
                },
                pill_fill: hex(theme.results.pill_fill),
            };
            top += height + gap;
            row
        })
        .collect();
    (label, rows)
}

/// The names the pinned slots' chords are recorded under, and the
/// reference key groups they pair with, slot by slot.
const SLOT_CHORDS: [&str; 5] = [
    "quick-slot-1",
    "quick-slot-2",
    "quick-slot-3",
    "quick-slot-4",
    "quick-slot-5",
];
const SLOT_GROUPS: [&str; 5] = ["pinned-1", "pinned-2", "pinned-3", "pinned-4", "pinned-5"];

/// Moves `group`, caps and all, so it begins at `x`.
fn shift_group(group: &mut KeyGroupRecord, x: f32) {
    let shift = x - group.rect.x;
    group.rect.x += shift;
    for cap in &mut group.caps {
        cap.rect.x += shift;
    }
}

/// Where the pinned home's chords lie, as `crate::ui::pinned` lays them
/// out: the label's at its right padding, centered in the label below its
/// top padding; each occupied slot's compact one in its top right corner,
/// the slot's inset from both edges.
fn declare_home_keys(window: &Window, theme: &Theme, home: &mut DeclaredPinned) {
    let geometry = &theme.geometry;
    let height = f32::from(geometry.keycap_height);
    let content = f32::from(geometry.section_height) - f32::from(geometry.section_padding_top);
    let y = home.label.y + f32::from(geometry.section_padding_top) + (content - height) / 2.;
    let keys = crate::keyboard::quick_slots_keys(numbered_pins(home.slots.len()));
    // The caps sit in the section label, whose tracking their labels
    // inherit, as the reference's `.kbd` inherits `.label`'s.
    let tracking = theme.typography.section_size * theme.typography.section_tracking;
    let mut label = declared_keys(
        window,
        theme,
        ("quick-slots", &keys),
        Some("pinned-label"),
        CapStyle::Regular,
        (0., y),
        Some(tracking),
    );
    let right = home.label.x + home.label.width - f32::from(geometry.section_padding_x);
    let left = right - label.rect.width;
    shift_group(&mut label, left);
    home.label_keys = Some(label);
    let inset = f32::from(geometry.pinned.keys_inset);
    for slot in &mut home.slots {
        let width = shaped_width(
            window,
            theme,
            slot.title,
            theme.typography.slot_title_size,
            theme.typography.medium,
        );
        slot.truncated = Some(width > slot.title_rect.width);
        // Only the numbered pins have a chord.
        let index = slot.number - 1;
        if index >= SLOT_CHORDS.len() {
            continue;
        }
        let keys = crate::keyboard::quick_slot_keys(slot.number);
        let mut group = declared_keys(
            window,
            theme,
            (SLOT_CHORDS[index], &keys),
            Some(SLOT_GROUPS[index]),
            CapStyle::Compact,
            (0., slot.rect.y + inset),
            None,
        );
        let left = slot.rect.x + slot.rect.width - inset - group.rect.width;
        shift_group(&mut group, left);
        slot.key_group = Some(group);
    }
}

fn parse_binding(binding: &str) -> Binding {
    Binding::parse(binding).unwrap_or_else(|why| panic!("keycap fixture binding {binding}: {why}"))
}

fn style_name(style: CapStyle) -> &'static str {
    match style {
        CapStyle::Regular => "regular",
        CapStyle::Compact => "compact",
        CapStyle::Accent => "accent",
    }
}

/// `binding`'s key sequence in `style`, laid out from `(x, y)` by the
/// rules [`keycap::key_sequence`] draws it with: each cap its label's
/// shaped width plus its padding, at least as wide as it is high, the
/// caps `key_gap` apart.
/// Each cap's label width and its own width, as the production keycap
/// lays `keys` out in `style`: the label's shaped width in Geist Mono
/// 500 and the cap's padding, at least as wide as the cap is tall.
fn cap_widths(
    window: &Window,
    theme: &Theme,
    keys: &keycap::KeySequence,
    style: CapStyle,
    spacing: Option<Pixels>,
) -> Vec<(f32, f32)> {
    let CapMetrics {
        height,
        padding_x: padding,
        text_size: size,
    } = style.metrics(theme);
    keys.keys
        .iter()
        .map(|key| {
            let label_width = shaped_width_tracked(
                window,
                theme.typography.mono_family.clone(),
                theme,
                &key.cap,
                size,
                theme.typography.medium,
                spacing,
            );
            let width = (label_width + 2. * f32::from(padding)).max(f32::from(height));
            (label_width, width)
        })
        .collect()
}

/// The width `keys` lays out to in `style`: its caps and the gaps between
/// them.
fn keys_width(window: &Window, theme: &Theme, keys: &keycap::KeySequence, style: CapStyle) -> f32 {
    let widths = cap_widths(window, theme, keys, style, None);
    let gaps = widths.len().saturating_sub(1) as f32 * f32::from(theme.geometry.key_gap);
    widths.iter().map(|(_, width)| width).sum::<f32>() + gaps
}

/// The width `keys` lays out to in `style`, as [`keys_width`] but with
/// each cap's label box rounded up to a whole pixel, as GPUI's text
/// element lays it out: where a chain of caps and labels places a part.
fn laid_keys_width(
    window: &Window,
    theme: &Theme,
    keys: &keycap::KeySequence,
    style: CapStyle,
) -> f32 {
    let CapMetrics {
        height, padding_x, ..
    } = style.metrics(theme);
    let widths = cap_widths(window, theme, keys, style, None);
    let gaps = widths.len().saturating_sub(1) as f32 * f32::from(theme.geometry.key_gap);
    widths
        .iter()
        .map(|(label, _)| (label.ceil() + 2. * f32::from(padding_x)).max(f32::from(height)))
        .sum::<f32>()
        + gaps
}

fn declared_group(
    window: &Window,
    theme: &Theme,
    (id, binding): (&'static str, &Binding),
    group: Option<&'static str>,
    style: CapStyle,
    at: (f32, f32),
) -> KeyGroupRecord {
    let keys = crate::keyboard::binding_keys(binding);
    declared_keys(window, theme, (id, &keys), group, style, at, None)
}

/// `keys` in `style`, laid out from `(x, y)` as [`declared_group`] lays
/// out a binding's: for a sequence that is not one binding's (the pinned
/// label's "Ctrl" "1–5"). `spacing` is the tracking the caps' labels
/// inherit from where they sit, if any (the section label's).
fn declared_keys(
    window: &Window,
    theme: &Theme,
    (id, keys): (&'static str, &keycap::KeySequence),
    group: Option<&'static str>,
    style: CapStyle,
    (x, y): (f32, f32),
    spacing: Option<Pixels>,
) -> KeyGroupRecord {
    let geometry = &theme.geometry;
    let typography = &theme.typography;
    let CapMetrics {
        height,
        text_size: size,
        ..
    } = style.metrics(theme);
    let mut left = x;
    let mut caps = Vec::new();
    for (key, (label_width, width)) in keys
        .keys
        .iter()
        .zip(cap_widths(window, theme, keys, style, spacing))
    {
        caps.push(CapRecord {
            label: key.cap.to_string(),
            label_width,
            rect: Rect {
                x: left,
                y,
                width,
                height: f32::from(height),
            },
        });
        left += width + f32::from(geometry.key_gap);
    }
    let width = left - f32::from(geometry.key_gap) - x;
    KeyGroupRecord {
        binding: id,
        group,
        style: style_name(style),
        name: keys.name(),
        font: format!(
            "{}px {} {}",
            f32::from(size),
            typography.mono_family,
            typography.medium.0
        ),
        rect: Rect {
            x,
            y,
            width,
            height: f32::from(height),
        },
        caps,
    }
}

/// The Settings shell's layout in a client of `client` size, as
/// `ui::settings_shell` lays it out (see [`settings_frame`]).
struct SettingsFrame {
    titlebar: Rect,
    sidebar: Rect,
    /// The search field's well.
    search: Rect,
    /// The section items, top to bottom.
    sections: Vec<Rect>,
    page: Rect,
    /// The heading's line box, at the top of the controls column.
    heading: Rect,
    /// Where the subtitle's first line begins, under the heading.
    subtitle_top: f32,
    /// The preview column's caption's line box, where the page holds both
    /// columns side by side; `None` where they collapse.
    aside: Option<Rect>,
}

/// The Settings shell's layout in a client of `client` size, from the
/// theme's tokens: the titlebar across the top, the sidebar below it with
/// its search well and its sections inside its padding and its 1px rule,
/// and the page beside it — its heading at its padding, and its two
/// columns side by side where the page holds both.
fn settings_frame(theme: &Theme, client: (f32, f32)) -> SettingsFrame {
    let settings = &theme.geometry.settings;
    let typography = &theme.typography;
    let f = f32::from;
    let (width, height) = client;
    let titlebar = Rect {
        x: 0.,
        y: 0.,
        width,
        height: f(settings.titlebar_height),
    };
    let sidebar = Rect {
        x: 0.,
        y: titlebar.height,
        width: f(settings.sidebar_width),
        height: height - titlebar.height,
    };
    let inner_x = f(settings.sidebar_padding_x);
    let inner_width = sidebar.width - 1. - 2. * inner_x;
    let search = Rect {
        x: inner_x,
        y: sidebar.y + f(settings.sidebar_padding_y),
        width: inner_width,
        height: f(settings.search_height),
    };
    let below_search = search.y + search.height + f(settings.search_margin_bottom);
    let first = below_search + f(settings.sidebar_gap);
    let step = f(settings.item_height) + f(settings.sidebar_gap);
    let sections = (0..SETTINGS_SECTIONS.len())
        .map(|index| Rect {
            x: inner_x,
            y: first + index as f32 * step,
            width: inner_width,
            height: f(settings.item_height),
        })
        .collect();
    let page = Rect {
        x: sidebar.width,
        y: sidebar.y,
        width: width - sidebar.width,
        height: sidebar.height,
    };
    let left = page.x + f(settings.page_padding_x);
    let room = page.width - 2. * f(settings.page_padding_x);
    let gap = f(settings.column_gap);
    let aside_width = f(settings.aside_width);
    let side_by_side = room >= f(settings.controls_width) + gap + aside_width;
    let column = if side_by_side {
        room - gap - aside_width
    } else {
        room
    };
    let top = page.y + f(settings.page_padding_top);
    let heading = Rect {
        x: left,
        y: top,
        width: column,
        height: f(typography.settings.heading.line_height),
    };
    let aside = side_by_side.then(|| Rect {
        x: left + column + gap,
        y: top,
        width: aside_width,
        height: f(typography.settings.caption.line_height),
    });
    SettingsFrame {
        titlebar,
        sidebar,
        search,
        sections,
        page,
        subtitle_top: heading.y + heading.height + f(settings.header_gap),
        heading,
        aside,
    }
}

/// The Settings shell as the manifest declares it: where each part lies,
/// which section the pointer is over in each capture, and the colors the
/// shell paints with.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclaredSettings {
    titlebar: Rect,
    /// The titlebar's label: centered over the window above its rule.
    title: DeclaredText,
    sidebar: Rect,
    /// The search field's well.
    search: Rect,
    /// The placeholder's text box, after the magnifier, its gap and the
    /// field's text inset.
    placeholder: DeclaredText,
    sections: Vec<DeclaredNavItem>,
    page: Rect,
    /// The heading's line box, and the subtitle's first line.
    heading: DeclaredText,
    subtitle: DeclaredText,
    /// The preview column's caption, where the columns sit side by side.
    #[serde(skip_serializing_if = "Option::is_none")]
    aside: Option<DeclaredText>,
    hovered: Vec<DeclaredHover>,
    colors: SettingsColors,
}

/// One section item as the manifest declares it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclaredNavItem {
    label: &'static str,
    rect: Rect,
    selected: bool,
    /// The 16px glyph's box, and whether the glyph is the board's own path
    /// (see [`FixtureSection`]).
    glyph: Rect,
    board_glyph: bool,
    /// The label's text box: after the glyph and the gap, its shaped width.
    label_box: Rect,
    /// The count at the item's right end, if it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<DeclaredText>,
}

/// Which section the pointer is over while a capture is taken.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclaredHover {
    capture: &'static str,
    section: Option<usize>,
}

/// The colors the Settings shell paints with, as the theme in force holds
/// them.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsColors {
    settings_tint: Hex,
    sidebar_fill: Hex,
    nav_text: Hex,
    nav_hover: Hex,
    nav_hover_text: Hex,
    nav_selected: Hex,
    nav_selected_text: Hex,
    nav_icon: Hex,
    field_fill: Hex,
    field_edge: Hex,
    heading_text: Hex,
    text_body: Hex,
    text_muted: Hex,
    text_placeholder: Hex,
    hairline_soft: Hex,
}

/// The Settings shell the scenario renders, declared: the layout of
/// [`settings_frame`] with the widths the text system shapes, and each
/// pointer step resolved to the center of the section it names — which
/// `steps` and `captures` take on.
fn declared_settings(
    window: &Window,
    theme: &Theme,
    (client, selected): ((f32, f32), usize),
    steps: &mut [ResolvedStep],
    captures: &mut [DeclaredCapture],
) -> DeclaredSettings {
    let frame = settings_frame(theme, client);
    let settings = &theme.geometry.settings;
    let typography = &theme.typography;
    let f = f32::from;
    let text_size = typography.settings_text_size;
    let caption_size = typography.settings_caption_size;
    let line = |size: Pixels| f(size) * typography.line_height;
    // The label, centered over the window in the titlebar above its rule.
    let title_width = shaped_width(window, theme, "Settings", text_size, typography.medium);
    let above_rule = frame.titlebar.height - 1.;
    let title = DeclaredText {
        text: "Settings".into(),
        rect: Rect {
            x: (client.0 - title_width) / 2.,
            y: (above_rule - line(text_size)) / 2.,
            width: title_width,
            height: line(text_size),
        },
    };
    let placeholder = DeclaredText {
        text: settings_shell::SEARCH_PLACEHOLDER.into(),
        rect: Rect {
            x: frame.search.x
                + f(settings.search_padding_x)
                + f(settings.search_glyph)
                + f(settings.search_gap)
                + f(theme.geometry.search_text_inset),
            y: frame.search.y,
            width: shaped_width(
                window,
                theme,
                settings_shell::SEARCH_PLACEHOLDER,
                text_size,
                typography.regular,
            ),
            height: frame.search.height,
        },
    };
    let glyph = f(settings.item_glyph);
    let sections = SETTINGS_SECTIONS
        .iter()
        .zip(&frame.sections)
        .enumerate()
        .map(|(index, (section, &rect))| {
            let left = rect.x + f(settings.item_padding_x);
            let label = section.label;
            let label_width = shaped_width(window, theme, label, text_size, typography.medium);
            let count = section.count.map(|count| {
                let width = shaped_width(window, theme, count, caption_size, typography.medium);
                DeclaredText {
                    text: count.into(),
                    rect: Rect {
                        x: rect.x + rect.width - f(settings.item_padding_x) - width,
                        y: rect.y,
                        width,
                        height: rect.height,
                    },
                }
            });
            DeclaredNavItem {
                label,
                rect,
                selected: index == selected,
                glyph: Rect {
                    x: left,
                    y: rect.y + (rect.height - glyph) / 2.,
                    width: glyph,
                    height: glyph,
                },
                board_glyph: section.board_glyph,
                label_box: Rect {
                    x: left + glyph + f(settings.item_gap),
                    y: rect.y,
                    width: label_width,
                    height: rect.height,
                },
                count,
            }
        })
        .collect();
    let heading = DeclaredText {
        text: SETTINGS_HEADING.into(),
        rect: frame.heading,
    };
    let subtitle = DeclaredText {
        text: SETTINGS_SUBTITLE.into(),
        rect: Rect {
            y: frame.subtitle_top,
            height: f(typography.settings.subtitle.line_height),
            ..frame.heading
        },
    };
    let aside = frame.aside.map(|rect| DeclaredText {
        text: SETTINGS_ASIDE.into(),
        rect,
    });
    // The pointer's steps land on the centers of the sections they name,
    // and each capture records the section the pointer is over.
    let mut pointer = None;
    let mut over = None;
    let mut hovered = Vec::new();
    for step in steps.iter_mut() {
        match step.step {
            Step::Pointer { row, nudge } => {
                pointer = frame.sections.get(row).map(|rect| {
                    let (x, y) = rect.center();
                    (x + f32::from(nudge), y)
                });
                over = pointer.map(|_| row);
                step.point = pointer;
            }
            Step::Capture { name } => {
                if let Some(capture) = captures.iter_mut().find(|capture| capture.name == name) {
                    capture.pointer = pointer;
                }
                hovered.push(DeclaredHover {
                    capture: name,
                    section: over,
                });
            }
            _ => {}
        }
    }
    DeclaredSettings {
        titlebar: frame.titlebar,
        title,
        sidebar: frame.sidebar,
        search: frame.search,
        placeholder,
        sections,
        page: frame.page,
        heading,
        subtitle,
        aside,
        hovered,
        colors: SettingsColors {
            settings_tint: Hex(theme.settings_tint),
            sidebar_fill: Hex(theme.sidebar_fill),
            nav_text: Hex(theme.nav_text),
            nav_hover: Hex(theme.nav_hover),
            nav_hover_text: Hex(theme.nav_hover_text),
            nav_selected: Hex(theme.nav_selected),
            nav_selected_text: Hex(theme.nav_selected_text),
            nav_icon: Hex(theme.nav_icon),
            field_fill: Hex(theme.field_fill),
            field_edge: Hex(theme.field_edge),
            heading_text: Hex(theme.heading_text),
            text_body: Hex(theme.text_body),
            text_muted: Hex(theme.text_muted),
            text_placeholder: Hex(theme.text_placeholder),
            hairline_soft: Hex(theme.hairline_soft),
        },
    }
}

// ------------------------------------------------- the Appearance page (#98)

/// The Appearance page as a capture declares it (#98): where each part
/// lies, in the state the capture shows, with the colors it is drawn in.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclaredAppearance {
    capture: &'static str,
    /// "board" (the reference's page) or "production" (Pane's).
    variant: &'static str,
    /// The controls column.
    column: Rect,
    /// The field labels, top to bottom.
    labels: Vec<DeclaredLine>,
    /// The segmented choices.
    tracks: Vec<DeclaredTrack>,
    /// The fields' descriptions, notices and notes: each first line's box.
    descriptions: Vec<DeclaredLine>,
    swatches: Vec<DeclaredSwatch>,
    sliders: Vec<DeclaredSlider>,
    toggles: Vec<DeclaredToggle>,
    /// The preview, where the page holds it beside the controls.
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<DeclaredMiniature>,
    /// The board's link under the preview.
    #[serde(skip_serializing_if = "Option::is_none")]
    link: Option<DeclaredText>,
    colors: AppearanceColors,
}

/// A line of text: what it says, its first line's box, its color and
/// the opacity its field is drawn at.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclaredLine {
    text: String,
    rect: Rect,
    color: Hex,
    opacity: f32,
}

/// A segmented choice: its track, the opacity its field is drawn at, and
/// its segments.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclaredTrack {
    field: &'static str,
    rect: Rect,
    opacity: f32,
    segments: Vec<DeclaredSegment>,
}

/// One segment: its label, the target a step names it by, its box, its
/// label's line box (centered in it), and its state.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclaredSegment {
    label: &'static str,
    target: String,
    rect: Rect,
    label_box: Rect,
    chosen: bool,
    hovered: bool,
}

/// One accent swatch: its disc (the chosen one's rings reach 4px past it).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclaredSwatch {
    name: &'static str,
    rect: Rect,
    chosen: bool,
}

/// One slider: its label, its value (right-aligned), its box and its
/// field's opacity.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclaredSlider {
    label: DeclaredLine,
    value: DeclaredText,
    rect: Rect,
    opacity: f32,
}

/// One toggle row: its label's line box, the toggle and its knob.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclaredToggle {
    label: DeclaredText,
    rect: Rect,
    knob: Rect,
    on: bool,
}

/// The preview: the column's caption, the stage, the miniature and its
/// parts, and whether it shows the glass tint.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclaredMiniature {
    caption: Rect,
    stage: Rect,
    panel: Rect,
    search: Rect,
    query: DeclaredText,
    caret: Rect,
    pins: Vec<Rect>,
    rows: Vec<DeclaredMiniRow>,
    footer: Rect,
    #[serde(skip_serializing_if = "Option::is_none")]
    tip: Option<DeclaredText>,
    keys: Rect,
    glass: bool,
}

/// One row of the miniature: its title's and kind's line boxes, its box,
/// its tile and whether it is selected.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclaredMiniRow {
    title: DeclaredText,
    kind: DeclaredText,
    rect: Rect,
    tile: Rect,
    selected: bool,
}

/// The colors the Appearance page paints with, as the theme in force
/// holds them.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppearanceColors {
    segment_track: Hex,
    segment_edge: Hex,
    segment_on: Hex,
    segment_text: Hex,
    segment_hover_text: Hex,
    segment_on_text: Hex,
    title: Hex,
    muted: Hex,
    body: Hex,
    query: Hex,
    accent: Hex,
    footer_tint: Hex,
    preview_row_selected: Hex,
    preview_pin: Hex,
    preview_stage_edge: Hex,
}

/// The state an Appearance capture shows: the board's chosen material (an
/// index into [`BOARD_MATERIALS`]) and the segment under the pointer.
#[derive(Clone, Copy, Debug, Default)]
struct AppearanceState {
    material: usize,
    hovered: Option<&'static str>,
}

/// How many lines `text` wraps to in `width` at `size` and `weight`, word
/// by word, as the text system wraps a paragraph.
fn wrapped_lines(
    window: &Window,
    theme: &Theme,
    text: &str,
    (size, weight): (Pixels, FontWeight),
    width: f32,
) -> usize {
    let mut lines = 1;
    let mut line = String::new();
    for word in text.split(' ') {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && shaped_width(window, theme, &candidate, size, weight) > width {
            lines += 1;
            line = word.to_owned();
        } else {
            line = candidate;
        }
    }
    lines
}

/// `count` segments in `track`, as `ui::controls` lays them out: equal
/// shares of the track inside its padding, the segment gap between them.
fn segment_rects(theme: &Theme, track: Rect, count: usize) -> Vec<Rect> {
    let controls = &theme.geometry.controls;
    let padding = f32::from(controls.track_padding);
    let gap = f32::from(controls.segment_gap);
    let width = (track.width - 2. * padding - (count as f32 - 1.) * gap) / count as f32;
    (0..count)
        .map(|index| Rect {
            x: track.x + padding + index as f32 * (width + gap),
            y: track.y + padding,
            width,
            height: f32::from(controls.segment_height),
        })
        .collect()
}

/// The preview's geometry from the aside's top left, as `ui::preview`
/// lays it out: the caption, the stage, the miniature centered on it, its
/// search line, its pinned slots (`pins` of them), its rows (`rows`) and
/// its footer.
struct MiniatureFrame {
    caption: Rect,
    stage: Rect,
    panel: Rect,
    search: Rect,
    pins: Vec<Rect>,
    rows: Vec<Rect>,
    footer: Rect,
}

fn miniature_frame(theme: &Theme, (x, y): (f32, f32), pins: usize, rows: usize) -> MiniatureFrame {
    let settings = &theme.geometry.settings;
    let preview = &theme.geometry.preview;
    let f = f32::from;
    let caption = Rect {
        x,
        y,
        width: f(settings.aside_width),
        height: f(theme.typography.settings.caption.line_height),
    };
    let stage = Rect {
        y: y + caption.height + f(settings.aside_gap),
        height: f(preview.stage_height),
        ..caption
    };
    let panel_x = stage.x + (stage.width - f(preview.panel_width)) / 2.;
    let panel_y = stage.y + f(preview.stage_padding_top);
    let search = Rect {
        x: panel_x,
        y: panel_y,
        width: f(preview.panel_width),
        height: f(preview.search_height),
    };
    let inner_x = panel_x + f(preview.body_padding);
    let inner_width = search.width - 2. * f(preview.body_padding);
    let mut top = search.y + search.height + f(preview.body_padding);
    let mut pin_rects = Vec::new();
    if pins > 0 {
        let padding = f(preview.pins_padding_x);
        let gap = f(preview.pins_gap);
        let width = (inner_width - 2. * padding - (pins as f32 - 1.) * gap) / pins as f32;
        pin_rects = (0..pins)
            .map(|index| Rect {
                x: inner_x + padding + index as f32 * (width + gap),
                y: top + f(preview.pins_padding_top),
                width,
                height: f(preview.pin_height),
            })
            .collect();
        top += f(preview.pins_padding_top)
            + f(preview.pin_height)
            + f(preview.pins_padding_bottom)
            + f(preview.body_gap);
    }
    let step = f(preview.row_height) + f(preview.body_gap);
    let row_rects: Vec<Rect> = (0..rows)
        .map(|index| Rect {
            x: inner_x,
            y: top + index as f32 * step,
            width: inner_width,
            height: f(preview.row_height),
        })
        .collect();
    let rows_bottom = row_rects.last().map_or(top, |row| row.y + row.height);
    let body_bottom = rows_bottom + f(preview.body_padding);
    let footer = Rect {
        x: panel_x,
        y: body_bottom,
        width: search.width,
        height: f(preview.footer_height),
    };
    MiniatureFrame {
        caption,
        stage,
        panel: Rect {
            height: footer.y + footer.height - panel_y,
            ..search
        },
        search,
        pins: pin_rects,
        rows: row_rects,
        footer,
    }
}

/// The miniature showing `content` from the aside's top left `origin`,
/// with its text placed by the widths the text system shapes.
fn declared_miniature(
    window: &Window,
    theme: &Theme,
    origin: (f32, f32),
    content: &PreviewContent,
    glass: bool,
) -> DeclaredMiniature {
    let preview = &theme.geometry.preview;
    let typography = &theme.typography;
    let lines = &typography.settings;
    let f = f32::from;
    let frame = miniature_frame(theme, origin, content.pins.len(), content.rows.len());
    // A line box of `size` centered in a box `height` high from `top`.
    let centered = |top: f32, height: f32, line: Pixels| top + (height - f(line)) / 2.;
    // The search line's content sits above its 1px rule.
    let search_inner = frame.search.height - 1.;
    let query_width = shaped_width(
        window,
        theme,
        &content.query,
        lines.preview_query.size,
        typography.regular,
    );
    let query = DeclaredText {
        text: content.query.to_string(),
        rect: Rect {
            x: frame.search.x
                + f(preview.search_padding_x)
                + f(preview.search_glyph)
                + f(preview.search_gap),
            y: centered(
                frame.search.y,
                search_inner,
                lines.preview_query.line_height,
            ),
            width: query_width,
            height: f(lines.preview_query.line_height),
        },
    };
    let caret = Rect {
        x: query.rect.x + query_width + f(preview.search_gap) - f(preview.caret_pull),
        y: centered(frame.search.y, search_inner, preview.caret_height),
        width: f(preview.caret_width),
        height: f(preview.caret_height),
    };
    let rows = content
        .rows
        .iter()
        .zip(&frame.rows)
        .map(|(row, &rect)| {
            let tile = f(preview.row_tile.size);
            let title_width = shaped_width(
                window,
                theme,
                &row.title,
                lines.preview_title.size,
                typography.medium,
            );
            let kind_width = shaped_width(
                window,
                theme,
                &row.kind,
                lines.preview_small.size,
                typography.regular,
            );
            let left = rect.x + f(preview.row_padding_x);
            DeclaredMiniRow {
                title: DeclaredText {
                    text: row.title.to_string(),
                    rect: Rect {
                        x: left + tile + f(preview.row_gap),
                        y: centered(rect.y, rect.height, lines.preview_title.line_height),
                        width: title_width,
                        height: f(lines.preview_title.line_height),
                    },
                },
                kind: DeclaredText {
                    text: row.kind.to_string(),
                    rect: Rect {
                        x: rect.x + rect.width - f(preview.row_padding_x) - kind_width,
                        y: centered(rect.y, rect.height, lines.preview_small.line_height),
                        width: kind_width,
                        height: f(lines.preview_small.line_height),
                    },
                },
                rect,
                tile: Rect {
                    x: left,
                    y: centered(rect.y, rect.height, preview.row_tile.size),
                    width: tile,
                    height: tile,
                },
                selected: row.selected,
            }
        })
        .collect();
    // The footer's content sits below its 1px rule.
    let footer_top = frame.footer.y + 1.;
    let footer_inner = frame.footer.height - 1.;
    let tip = content.tip.as_ref().map(|tip| DeclaredText {
        text: tip.to_string(),
        rect: Rect {
            x: frame.footer.x + f(preview.footer_padding_left),
            y: centered(footer_top, footer_inner, lines.preview_small.line_height),
            width: shaped_width(
                window,
                theme,
                tip,
                lines.preview_small.size,
                typography.regular,
            ),
            height: f(lines.preview_small.line_height),
        },
    });
    let keys_wide = keys_width(window, theme, &content.keys, CapStyle::Accent);
    let keys = Rect {
        x: frame.footer.x + frame.footer.width - f(preview.footer_padding_right) - keys_wide,
        y: centered(footer_top, footer_inner, theme.geometry.keycap_height),
        width: keys_wide,
        height: f(theme.geometry.keycap_height),
    };
    DeclaredMiniature {
        caption: frame.caption,
        stage: frame.stage,
        panel: frame.panel,
        search: frame.search,
        query,
        caret,
        pins: frame.pins,
        rows,
        footer: frame.footer,
        tip,
        keys,
        glass,
    }
}

/// Lays the Appearance page's controls column out, top to bottom, as
/// `ui::controls` stacks it: a cursor down the column that each part
/// advances past itself.
struct AppearanceColumn<'a> {
    window: &'a Window,
    theme: &'a Theme,
    x: f32,
    width: f32,
    y: f32,
    labels: Vec<DeclaredLine>,
    tracks: Vec<DeclaredTrack>,
    descriptions: Vec<DeclaredLine>,
}

impl AppearanceColumn<'_> {
    /// A field's label at `opacity`, then the field's gap.
    fn label(&mut self, text: &str, opacity: f32) {
        let typography = &self.theme.typography;
        let line = typography.settings.field_label;
        let width = shaped_width(self.window, self.theme, text, line.size, typography.medium);
        self.labels.push(DeclaredLine {
            text: text.into(),
            rect: Rect {
                x: self.x,
                y: self.y,
                width,
                height: f32::from(line.line_height),
            },
            color: Hex(self.theme.text_title),
            opacity,
        });
        self.y += f32::from(line.line_height) + f32::from(self.theme.geometry.controls.field_gap);
    }

    /// A segmented choice of `labels` for `field`, the one at `chosen`
    /// chosen, the one named `hovered` under the pointer.
    fn track(
        &mut self,
        field: &'static str,
        labels: &[&'static str],
        (chosen, hovered): (usize, Option<&'static str>),
        opacity: f32,
    ) {
        let theme = self.theme;
        let controls = &theme.geometry.controls;
        let line = theme.typography.settings.segment;
        let rect = Rect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: f32::from(controls.segment_height) + 2. * f32::from(controls.track_padding),
        };
        let segments = labels
            .iter()
            .zip(segment_rects(theme, rect, labels.len()))
            .enumerate()
            .map(|(index, (&label, segment))| {
                let width = shaped_width(
                    self.window,
                    theme,
                    label,
                    line.size,
                    theme.typography.medium,
                );
                DeclaredSegment {
                    label,
                    target: segment_target(label),
                    rect: segment,
                    label_box: Rect {
                        x: segment.x + (segment.width - width) / 2.,
                        y: segment.y + (segment.height - f32::from(line.line_height)) / 2.,
                        width,
                        height: f32::from(line.line_height),
                    },
                    chosen: index == chosen,
                    hovered: hovered == Some(label),
                }
            })
            .collect();
        self.tracks.push(DeclaredTrack {
            field,
            rect,
            opacity,
            segments,
        });
        self.y += rect.height;
    }

    /// A description under a control (the field's gap above it), or a
    /// notice of its own (`gap` false), in `color` at `opacity`.
    fn description(&mut self, text: &str, color: Hsla, opacity: f32, gap: bool) {
        let theme = self.theme;
        let line = theme.typography.settings.field_description;
        if gap {
            self.y += f32::from(theme.geometry.controls.field_gap);
        }
        let weight = (line.size, theme.typography.regular);
        let lines = wrapped_lines(self.window, theme, text, weight, self.width);
        self.descriptions.push(DeclaredLine {
            text: text.into(),
            rect: Rect {
                x: self.x,
                y: self.y,
                width: self.width,
                height: f32::from(line.line_height),
            },
            color: Hex(color),
            opacity,
        });
        self.y += lines as f32 * f32::from(line.line_height);
    }

    /// The gap between field groups.
    fn next_group(&mut self) {
        self.y += f32::from(self.theme.geometry.controls.group_gap);
    }
}

/// The Appearance page `page` shows in a client of `client` size, in
/// `state`, declared for the capture `capture`.
fn declared_appearance(
    window: &Window,
    theme: &Theme,
    (client, page): ((f32, f32), AppearancePage),
    (capture, state): (&'static str, AppearanceState),
    cx: &App,
) -> DeclaredAppearance {
    let frame = settings_frame(theme, client);
    let settings_geometry = &theme.geometry.settings;
    let controls = &theme.geometry.controls;
    let lines = &theme.typography.settings;
    let f = f32::from;
    let disabled = controls.disabled_opacity;
    let mut column = AppearanceColumn {
        window,
        theme,
        x: frame.heading.x,
        width: frame.heading.width,
        y: frame.heading.y,
        labels: Vec::new(),
        tracks: Vec::new(),
        descriptions: Vec::new(),
    };
    // The heading block: the heading, its subtitle's lines, the block's
    // margin, then the column's gap.
    let subtitle = match page {
        AppearancePage::Board => SETTINGS_SUBTITLE,
        AppearancePage::Production { .. } => "",
    };
    let subtitle_style = (lines.subtitle.size, theme.typography.regular);
    let subtitle_lines = wrapped_lines(window, theme, subtitle, subtitle_style, column.width);
    column.y += f(lines.heading.line_height)
        + f(settings_geometry.header_gap)
        + subtitle_lines as f32 * f(lines.subtitle.line_height)
        + f(settings_geometry.header_margin_bottom);
    column.next_group();

    let mut swatches = Vec::new();
    let mut sliders = Vec::new();
    let mut toggles = Vec::new();
    let (content, glass, link) = match page {
        AppearancePage::Board => {
            let solid = state.material == BOARD_SOLID;
            let dimmed = if solid { disabled } else { 1. };
            // Material.
            let names = BOARD_MATERIALS.map(|(name, _)| name);
            column.label("Material", 1.);
            column.track("material", &names, (state.material, state.hovered), 1.);
            let note = BOARD_MATERIALS[state.material].1;
            column.description(note, theme.text_muted, 1., true);
            column.next_group();
            // Accent.
            column.label("Accent", 1.);
            let size = f(controls.swatch_size);
            let step = size + f(controls.swatch_gap);
            for (index, &(name, _)) in BOARD_SWATCHES.iter().enumerate() {
                swatches.push(DeclaredSwatch {
                    name,
                    rect: Rect {
                        x: column.x + index as f32 * step,
                        y: column.y,
                        width: size,
                        height: size,
                    },
                    chosen: index == 0,
                });
            }
            column.y += size;
            column.next_group();
            // Blur and tint.
            for (label, value, _) in [BOARD_BLUR, BOARD_TINT] {
                sliders.push(declared_slider(&mut column, (label, value), dimmed));
                if label == BOARD_TINT.0 {
                    column.description(BOARD_TINT_NOTE, theme.text_muted, dimmed, true);
                }
                column.next_group();
            }
            // Density.
            column.label("Density", 1.);
            let hovered = (BOARD_DENSITY, state.hovered);
            column.track("density", &BOARD_DENSITIES, hovered, 1.);
            column.next_group();
            // The toggles, each under its rule.
            for label in BOARD_TOGGLES {
                toggles.push(declared_toggle(&column, label));
                column.y += f(controls.toggle_row_height);
            }
            let content = board_preview(&settings::keyboard_of(cx));
            (content, !solid, Some(BOARD_LINK))
        }
        AppearancePage::Production { overridden } => {
            let production = ProductionAppearance::read(cx);
            let opacity = if overridden { disabled } else { 1. };
            if overridden {
                let notice = appearance::override_text(&production.overrides);
                column.description(&notice, theme.warning, 1., false);
                column.next_group();
            }
            let themes = appearance::THEMES.map(|(_, name, _)| name);
            let chosen = appearance::THEMES
                .iter()
                .position(|&(preference, ..)| preference == production.theme)
                .unwrap_or_default();
            column.label("Theme", opacity);
            column.track("theme", &themes, (chosen, None), opacity);
            column.next_group();
            let materials = appearance::MATERIALS.map(|(_, name, _)| name);
            let chosen = appearance::MATERIALS
                .iter()
                .position(|&(preference, ..)| preference == production.material)
                .unwrap_or_default();
            column.label("Material", opacity);
            column.track("material", &materials, (chosen, None), opacity);
            if let Some((note, color)) = appearance::material_note(production.material, theme) {
                column.description(&note, color, 1., true);
            }
            let glass = production.surface.is_glass();
            (production.preview, glass, None)
        }
    };
    let column_rect = Rect {
        x: column.x,
        y: frame.heading.y,
        width: column.width,
        height: column.y - frame.heading.y,
    };
    // The preview, beside the controls on the canonical page: a narrower
    // page wraps it below them, past what the capture shows, so it is not
    // declared there.
    let preview = frame
        .aside
        .map(|aside| declared_miniature(window, theme, (aside.x, aside.y), &content, glass));
    let link = link.zip(preview.as_ref()).map(|(text, preview)| {
        let line = lines.link;
        DeclaredText {
            text: text.into(),
            rect: Rect {
                x: preview.stage.x,
                y: preview.stage.y + preview.stage.height + f(settings_geometry.aside_gap),
                width: shaped_width(window, theme, text, line.size, theme.typography.medium),
                height: f(line.line_height),
            },
        }
    });
    let colors = &theme.controls;
    DeclaredAppearance {
        capture,
        variant: match page {
            AppearancePage::Board => "board",
            AppearancePage::Production { .. } => "production",
        },
        column: column_rect,
        labels: column.labels,
        tracks: column.tracks,
        descriptions: column.descriptions,
        swatches,
        sliders,
        toggles,
        preview,
        link,
        colors: AppearanceColors {
            segment_track: Hex(colors.segment_track),
            segment_edge: Hex(colors.segment_edge),
            segment_on: Hex(colors.segment_on),
            segment_text: Hex(colors.segment_text),
            segment_hover_text: Hex(colors.segment_hover_text),
            segment_on_text: Hex(colors.segment_on_text),
            title: Hex(theme.text_title),
            muted: Hex(theme.text_muted),
            body: Hex(theme.text_body),
            query: Hex(theme.text_query),
            accent: Hex(theme.accent),
            footer_tint: Hex(theme.footer_tint),
            preview_row_selected: Hex(colors.preview_row_selected),
            preview_pin: Hex(colors.preview_pin),
            preview_stage_edge: Hex(colors.preview_stage_edge),
        },
    }
}

/// A slider field at the column's cursor: its header (the label, the
/// value right-aligned on the label's line bottom) and its box, the field
/// drawn at `opacity`.
fn declared_slider(
    column: &mut AppearanceColumn<'_>,
    (label, value): (&str, &str),
    opacity: f32,
) -> DeclaredSlider {
    let theme = column.theme;
    let typography = &theme.typography;
    let lines = &typography.settings;
    let f = f32::from;
    let label_width = shaped_width(
        column.window,
        theme,
        label,
        lines.field_label.size,
        typography.medium,
    );
    let value_width = shaped_width_in(
        column.window,
        typography.mono_family.clone(),
        theme,
        value,
        lines.value.size,
        typography.regular,
    );
    let label_line = f(lines.field_label.line_height);
    let declared_label = DeclaredLine {
        text: label.into(),
        rect: Rect {
            x: column.x,
            y: column.y,
            width: label_width,
            height: label_line,
        },
        color: Hex(theme.text_title),
        opacity,
    };
    let value = DeclaredText {
        text: value.into(),
        rect: Rect {
            x: column.x + column.width - value_width,
            y: column.y + label_line - f(lines.value.line_height),
            width: value_width,
            height: f(lines.value.line_height),
        },
    };
    column.y += label_line + f(theme.geometry.controls.field_gap);
    let rect = Rect {
        x: column.x,
        y: column.y,
        width: column.width,
        height: f(theme.geometry.controls.slider_height),
    };
    column.y += rect.height;
    DeclaredSlider {
        label: declared_label,
        value,
        rect,
        opacity,
    }
}

/// A toggle row at the column's cursor, its toggle on: the label and the
/// toggle centered below the row's 1px rule, the knob in from the right.
fn declared_toggle(column: &AppearanceColumn<'_>, label: &'static str) -> DeclaredToggle {
    let theme = column.theme;
    let controls = &theme.geometry.controls;
    let line = theme.typography.settings.field_label;
    let f = f32::from;
    let inner = f(controls.toggle_row_height) - 1.;
    let top = column.y + 1.;
    let width = shaped_width(
        column.window,
        theme,
        label,
        line.size,
        theme.typography.regular,
    );
    let rect = Rect {
        x: column.x + column.width - f(controls.toggle_width),
        y: top + (inner - f(controls.toggle_height)) / 2.,
        width: f(controls.toggle_width),
        height: f(controls.toggle_height),
    };
    let knob = f(controls.toggle_knob);
    DeclaredToggle {
        label: DeclaredText {
            text: label.into(),
            rect: Rect {
                x: column.x,
                y: top + (inner - f(line.line_height)) / 2.,
                width,
                height: f(line.line_height),
            },
        },
        knob: Rect {
            x: rect.x + rect.width - f(controls.toggle_inset) - knob,
            y: rect.y + f(controls.toggle_inset),
            width: knob,
            height: knob,
        },
        rect,
        on: true,
    }
}

/// The Appearance page of `scenario` (`page`), capture by capture: each
/// point or click step resolved to the center of the segment it names —
/// which `steps` and `captures` take on — and the board's material
/// following its clicks.
fn declare_appearance(
    window: &Window,
    theme: &Theme,
    (scenario, page): (&Scenario, AppearancePage),
    steps: &mut [ResolvedStep],
    captures: &mut [DeclaredCapture],
    cx: &App,
) -> Vec<DeclaredAppearance> {
    let client = scenario.client;
    let mut state = AppearanceState::default();
    let mut pointer = None;
    let mut declared = Vec::new();
    for step in steps.iter_mut() {
        match step.step {
            Step::Point { target } | Step::Click { target } => {
                let layout = declared_appearance(window, theme, (client, page), ("", state), cx);
                let segment = layout
                    .tracks
                    .iter()
                    .flat_map(|track| &track.segments)
                    .find(|segment| segment.target == target);
                pointer = segment.map(|segment| segment.rect.center());
                step.point = pointer;
                state.hovered = segment.map(|segment| segment.label);
                if matches!(step.step, Step::Click { .. })
                    && let Some(index) = BOARD_MATERIALS
                        .iter()
                        .position(|&(name, _)| segment_target(name) == target)
                {
                    state.material = index;
                }
            }
            Step::Capture { name } => {
                if let Some(capture) = captures.iter_mut().find(|capture| capture.name == name) {
                    capture.pointer = pointer;
                }
                let layout = declared_appearance(window, theme, (client, page), (name, state), cx);
                declared.push(layout);
            }
            _ => {}
        }
    }
    declared
}

/// Every embedded face the theme names — Geist and Geist Mono at 400 and
/// 500, and Geist at 600 for the Settings headings — as the text system
/// resolves it, then a family no system has at each weight (the fallback,
/// for comparison).
fn font_resolution(window: &Window, theme: &Theme) -> Vec<FontResolution> {
    let typography = &theme.typography;
    let faces = [
        (typography.family.clone(), FontWeight::NORMAL),
        (typography.family.clone(), typography.medium),
        (typography.mono_family.clone(), FontWeight::NORMAL),
        (typography.mono_family.clone(), typography.medium),
        (typography.family.clone(), typography.heading_weight),
        (MISSING_FONT.into(), FontWeight::NORMAL),
        (MISSING_FONT.into(), typography.medium),
        (MISSING_FONT.into(), typography.heading_weight),
    ];
    faces
        .into_iter()
        .map(|(family, weight)| {
            let mut font = gpui::font(family.clone());
            font.weight = weight;
            FontResolution {
                font_id: window.text_system().resolve_font(&font).0,
                probe_width: shaped_width_in(
                    window,
                    family.clone(),
                    theme,
                    FONT_PROBE,
                    px(14.),
                    weight,
                ),
                family,
                weight: weight.0,
            }
        })
        .collect()
}

fn manifest(fixture: &FixtureWindow, window: &Window, cx: &App) -> Manifest {
    let visuals = settings::visuals(cx);
    // The manifest declares the unperturbed intent; the perturbation is
    // the rendering's fault the comparison must catch.
    let theme = visuals.theme;
    let geometry = &theme.geometry;
    let scenario = fixture.scenario;
    let frame = frame(&theme, scenario.client);

    let replay = replay(scenario, &theme);
    let mut captures = replay.captures;
    let mut steps = replay.steps;
    let mut keycaps = Vec::new();
    let mut tiles = Vec::new();
    let mut settings = None;
    let mut appearance = Vec::new();
    let mut pane_page = Vec::new();
    match scenario.family {
        Family::Settings => {
            let page = pages::pane_page(scenario.name);
            let mut shell = declared_settings(
                window,
                &theme,
                (
                    scenario.client,
                    pages::selected_section(page, SETTINGS_SELECTED),
                ),
                &mut steps,
                &mut captures,
            );
            if let Some(page) = page {
                // Pane's pages have no aside column (#99), and head their
                // column with their own heading and subtitle (none, for a
                // confirmation).
                shell.aside = None;
                if let Some((heading, subtitle)) = pages::heading(page) {
                    shell.heading.text = heading;
                    shell.subtitle.text = subtitle.unwrap_or_default();
                }
                pane_page = pages::declare(
                    window,
                    &theme,
                    (scenario.client, page),
                    &mut steps,
                    &mut captures,
                    cx,
                );
            }
            settings = Some(shell);
            if let Some(page) = appearance_page(scenario.name) {
                appearance = declare_appearance(
                    window,
                    &theme,
                    (scenario, page),
                    &mut steps,
                    &mut captures,
                    cx,
                );
            }
        }
        Family::Keycap => {
            let mut y = KEYCAP_INSET;
            for group in KEY_GROUPS {
                let record = declared_group(
                    window,
                    &theme,
                    (group.binding, &parse_binding(group.binding)),
                    group.group,
                    group.style,
                    (KEYCAP_INSET, y),
                );
                y += record.rect.height + KEYCAP_GAP;
                keycaps.push(record);
            }
        }
        Family::Tiles => {
            let mut x = KEYCAP_INSET;
            for &(size, tone, _) in TILES {
                let metrics = size.metrics(&theme);
                let side = f32::from(metrics.size);
                tiles.push(TileRecord {
                    size: match size {
                        TileSize::Row => "row",
                        TileSize::Slot => "slot",
                        TileSize::Mini => "mini",
                        TileSize::PreviewRow => "preview-row",
                        TileSize::PreviewPin => "preview-pin",
                    },
                    tone: if tone == IconTone::Command {
                        "command"
                    } else {
                        "app"
                    },
                    rect: Rect {
                        x,
                        y: KEYCAP_INSET,
                        width: side,
                        height: side,
                    },
                    glyph: f32::from(metrics.glyph),
                });
                x += side + TILE_GAP;
            }
        }
        Family::Root => {
            // The footer, as `ui::footer` lays it out: the mark and the
            // hint from the left padding; the Actions button at the right
            // padding, the rule, and the primary action before it — each
            // button centered in the strip's first line below the 1px rule.
            let rule = 1.;
            let line = frame.footer.height - rule;
            // Layout places the buttons on whole pixels: 7.5px of room
            // above a 34px button in the 49px line rounds to 8.
            let top =
                (frame.footer.y + rule + (line - f32::from(geometry.action_height)) / 2.).round();
            let cap_top =
                top + (f32::from(geometry.action_height) - f32::from(geometry.keycap_height)) / 2.;
            let keyboard = settings::keyboard_of(cx);
            let invoke = keyboard
                .binding(KeyboardAction::InvokeSelectedAction)
                .clone();
            let open = keyboard.binding(KeyboardAction::OpenActions).clone();
            let button_width = |label: &str, caps: f32| {
                2. * f32::from(geometry.action_padding_x)
                    + shaped_width(
                        window,
                        &theme,
                        label,
                        theme.typography.footer_size,
                        theme.typography.medium,
                    )
                    + f32::from(geometry.action_gap)
                    + caps
            };
            let right =
                frame.footer.x + frame.footer.width - f32::from(geometry.footer_padding_right);
            // The Actions button and its keys, then the rule before it.
            let mut actions_keys = declared_group(
                window,
                &theme,
                ("open-actions", &open),
                Some("footer-actions"),
                CapStyle::Regular,
                (0., cap_top),
            );
            let actions_width = button_width("Actions", actions_keys.rect.width);
            let actions_button = Rect {
                x: right - actions_width,
                y: top,
                width: actions_width,
                height: f32::from(geometry.action_height),
            };
            let shift = right - f32::from(geometry.action_padding_x) - actions_keys.rect.width;
            actions_keys.rect.x += shift;
            for cap in &mut actions_keys.caps {
                cap.rect.x += shift;
            }
            keycaps.push(actions_keys);
            let divider_height = f32::from(geometry.footer_divider_height);
            let divider = Rect {
                x: actions_button.x - f32::from(geometry.footer_buttons_gap) - 1.,
                y: frame.footer.y + rule + (line - divider_height) / 2.,
                width: 1.,
                height: divider_height,
            };
            let primary_right = divider.x - f32::from(geometry.footer_buttons_gap);
            // Laid out at 0 to measure it, then placed.
            let mut primary = declared_group(
                window,
                &theme,
                ("invoke", &invoke),
                Some("footer-primary"),
                CapStyle::Accent,
                (0., cap_top),
            );
            let cap_width = primary.rect.width;
            let mark_size = f32::from(geometry.footer_mark_size);
            let mark = Rect {
                x: frame.footer.x + f32::from(geometry.footer_padding_left),
                y: frame.footer.y + rule + (line - mark_size) / 2.,
                width: mark_size,
                height: mark_size,
            };
            for capture in &mut captures {
                capture.action_button = capture.action.map(|action| {
                    let width = button_width(action, cap_width);
                    Rect {
                        x: primary_right - width,
                        y: top,
                        width,
                        height: f32::from(geometry.action_height),
                    }
                });
                // The hint: its parts from after the mark, 6px apart.
                let parts = footer::hint_parts(
                    capture.actions_open,
                    crate::keyboard::binding_keys(&invoke),
                    crate::keyboard::binding_keys(&open),
                    crate::keyboard::escape_keys(),
                );
                let mut x = mark.x + mark_size + f32::from(geometry.footer_lead_gap);
                // The hint's room: up to the gap before the first button.
                let room = capture.action_button.map_or(divider.x, |button| button.x)
                    - f32::from(geometry.footer_lead_gap);
                let mut hint = Vec::new();
                for part in parts {
                    let (text, keys, width) = match part {
                        footer::HintPart::Text(text) => {
                            let width = shaped_width(
                                window,
                                &theme,
                                &text,
                                theme.typography.footer_size,
                                theme.typography.regular,
                            );
                            (text.to_string(), false, width)
                        }
                        // A sequence by its caps' labels, as the
                        // reference's DOM reads them ("CtrlK").
                        footer::HintPart::Keys(keys) => {
                            let width = keys_width(window, &theme, &keys, CapStyle::Regular);
                            let labels = keys.keys.iter().map(|key| key.cap.as_ref()).collect();
                            (labels, true, width)
                        }
                    };
                    hint.push(DeclaredHintPart {
                        text,
                        keys,
                        rect: Rect {
                            x,
                            y: frame.footer.y + rule,
                            width,
                            height: line,
                        },
                        clipped: x + width > room,
                    });
                    x += width + f32::from(geometry.footer_hint_gap);
                }
                capture.footer = Some(DeclaredFooter {
                    mark,
                    hint,
                    divider,
                    actions_button,
                    actions_pressed: capture.actions_open,
                });
            }
            let shift = primary_right - f32::from(geometry.action_padding_x) - cap_width;
            primary.rect.x += shift;
            for cap in &mut primary.caps {
                cap.rect.x += shift;
            }
            keycaps.push(primary);
            for capture in &mut captures {
                // An answer card has no trailing parts (#96).
                for row in capture.rows.iter_mut().filter(|row| !row.answer) {
                    declare_trailing(window, &theme, row);
                }
                if let Some(home) = capture.pinned.as_mut() {
                    declare_home_keys(window, &theme, home);
                }
            }
            if let Some(board) = result_board(scenario.name) {
                for capture in &mut captures {
                    declare_board(window, &theme, &frame, board, capture);
                }
            }
            // A click on the Actions button lands at its center.
            let (x, y) = actions_button.center();
            for step in &mut steps {
                if step.step
                    == (Step::Click {
                        target: ACTIONS_BUTTON,
                    })
                {
                    step.point = Some((x, y));
                }
            }
        }
        Family::Clipboard => {
            let keyboard = settings::keyboard_of(cx);
            declare_clipboard(window, &theme, &keyboard, &mut captures, &mut steps);
        }
        Family::Form => {
            pane_page = pages::declare(
                window,
                &theme,
                (scenario.client, pages::PanePage::Form),
                &mut steps,
                &mut captures,
                cx,
            );
        }
    }

    let bounds = window.bounds();
    let viewport = window.viewport_size();
    Manifest {
        kind: "pane-visual-fixture-manifest",
        scenario,
        perturbation: fixture.perturbation.map(Perturbation::name),
        material: match visuals.material.window_appearance() {
            gpui::WindowBackgroundAppearance::Opaque => "opaque",
            _ => "glass",
        },
        appearance: if theme.panel_solid.lightness < 0.5 {
            "dark"
        } else {
            "light"
        },
        window: WindowRecord {
            bounds: [
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            ],
            viewport: [f32::from(viewport.width), f32::from(viewport.height)],
            scale_factor: window.scale_factor(),
        },
        declared: DeclaredTokens {
            geometry: GeometryRecord {
                search_height: f32::from(geometry.search_height),
                search_padding_x: f32::from(geometry.search_padding_x),
                search_gap: f32::from(geometry.search_gap),
                search_size: f32::from(theme.typography.search_size),
                row_min_height: f32::from(geometry.row_min_height),
                row_radius: f32::from(geometry.row_radius),
                row_padding_x: f32::from(geometry.row_padding_x),
                row_gap: f32::from(geometry.row_gap),
                row_list_gap: f32::from(geometry.row_list_gap),
                row_title_size: f32::from(theme.typography.row_title_size),
                row_subtitle_size: f32::from(theme.typography.row_subtitle_size),
                list_padding_top: f32::from(geometry.list_padding_top),
                list_padding_bottom: f32::from(geometry.list_padding_bottom),
                list_padding_x: f32::from(geometry.list_padding_x),
                tile_size: f32::from(geometry.tile.size),
                tile_radius: f32::from(geometry.tile.radius),
                footer_height: f32::from(geometry.footer_height),
                footer_padding_left: f32::from(geometry.footer_padding_left),
                footer_padding_right: f32::from(geometry.footer_padding_right),
                action_height: f32::from(geometry.action_height),
                action_radius: f32::from(geometry.action_radius),
                action_padding_x: f32::from(geometry.action_padding_x),
                action_gap: f32::from(geometry.action_gap),
                keycap_height: f32::from(geometry.keycap_height),
                keycap_radius: f32::from(geometry.keycap_radius),
                keycap_padding_x: f32::from(geometry.keycap_padding_x),
                keycap_compact_height: f32::from(geometry.keycap_compact_height),
                keycap_compact_padding_x: f32::from(geometry.keycap_compact_padding_x),
                key_gap: f32::from(geometry.key_gap),
            },
            colors: ColorRecord {
                panel_solid: Hex(theme.panel_solid),
                keycap_background: Hex(theme.keycap_background),
                keycap_bottom: Hex(theme.keycap_bottom),
                alias_text: Hex(theme.alias_text),
                keycap_text: Hex(theme.keycap_text),
                accent: Hex(theme.accent),
                accent_ink: Hex(theme.accent_ink),
                panel_top_highlight: Hex(theme.panel_top_highlight),
                row_hover: Hex(theme.row_hover),
                row_selected: Hex(theme.row_selected),
                row_selected_border: Hex(theme.row_selected_border),
                footer_tint: Hex(theme.footer_tint),
                hairline: Hex(theme.hairline),
                hairline_soft: Hex(theme.hairline_soft),
                tile_background: Hex(theme.tile_background),
                tile_foreground: Hex(theme.tile_foreground),
                text_title: Hex(theme.text_title),
                text_muted: Hex(theme.text_muted),
                text_query: Hex(theme.text_query),
                text_placeholder: Hex(theme.text_placeholder),
                popover_solid: Hex(theme.popover_solid),
                popover_edge: Hex(theme.popover_edge),
                footer_mark: Hex(theme.footer_mark),
                footer_button_text: Hex(theme.footer_button_text),
                control_hover: Hex(theme.control_hover),
                footer_button_open: Hex(theme.footer_button_open),
                footer_divider: Hex(theme.footer_divider),
                action_selected: Hex(theme.action_selected),
                action_text: Hex(theme.action_text),
                action_icon: Hex(theme.action_icon),
                action_rule: Hex(theme.action_rule),
                actions_dimmer: Hex(theme.actions_dimmer),
                slot_background: Hex(theme.slot_background),
                slot_edge: Hex(theme.slot_edge),
                slot_hover: Hex(theme.slot_hover),
                slot_title: Hex(theme.slot_title),
            },
        },
        search_header: frame.search,
        list: frame.list,
        footer: frame.footer,
        captures,
        steps,
        keycaps,
        tiles,
        settings,
        appearance_page: appearance,
        pane_page,
        font_resolution: font_resolution(window, &theme),
        fonts: crate::ui::FONTS
            .iter()
            .map(|&(file, bytes)| FontRecord {
                file,
                bytes: bytes.len(),
                sha256: format!("{:X}", Sha256::digest(bytes)),
            })
            .collect(),
    }
}

/// Runs the fixture application: the production settings (in the
/// isolated data directory, or in memory), the embedded fonts, the
/// production key bindings, and the fixture window at the scenario's
/// client size. Writes the manifest once the window has drawn its first
/// frame, then runs until the window closes.
pub fn run(options: FixtureOptions) -> Result<(), String> {
    let scenario = find_scenario(&options.scenario)?;
    let FixtureOptions {
        data_dir,
        theme,
        material,
        perturbation,
        manifest: manifest_path,
        ..
    } = options;
    // A scenario that names its own appearance renders in it, whatever the
    // run's theme.
    let theme = scenario.theme.map(str::to_owned).or(theme);
    gpui_platform::application().run(move |cx: &mut App| {
        settings::init_with_overrides(
            data_dir.clone(),
            settings::Overrides::parse(theme.as_deref(), material.as_deref()),
            cx,
        );
        if let Err(error) = crate::ui::load_fonts(cx) {
            eprintln!("the fixture's fonts could not be loaded: {error:#}");
        }
        // The production bindings, as the launcher registers them: the
        // text editing keys, then the selection keys in the query field,
        // then the launcher's own (Back among them).
        let keyboard = settings::keyboard_of(cx);
        let text_editing = bind_text_editing(cx);
        root_search::bind_keys(cx, &text_editing, &keyboard);
        clipboard_history::bind_keys(cx, &text_editing, &keyboard);
        actions_panel::bind_keys(cx, &text_editing);
        crate::keyboard::bind_keys(cx, &keyboard, pane_core::NavigationBindings::None);

        let (width, height) = scenario.client;
        let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_background: settings::window_background(cx),
            // Shown without activation: a capture run never takes the
            // foreground from whatever the operator is doing. The capture
            // helper posts its input to this window and reads its client
            // with PrintWindow, neither of which needs the foreground.
            focus: false,
            titlebar: Some(TitlebarOptions {
                title: Some(format!("Pane visual fixture - {}", scenario.name).into()),
                appears_transparent: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let window = match cx.open_window(window_options, |window, cx| {
            #[cfg(target_os = "windows")]
            crate::prefer_rounded_window_corners(window);
            cx.new(|cx| {
                let fixture = FixtureWindow::new(scenario, perturbation, window, cx);
                // Root search opens with the query field focused; the
                // fixture does the same, so typing reaches the field the
                // moment the window appears. The Settings board's search
                // field is not focused (it would draw a caret the board
                // does not), and the Settings window opens with its
                // sections focused, not the field.
                if scenario.family != Family::Settings {
                    window.focus(&fixture.query.focus_handle(cx), cx);
                }
                fixture
            })
        }) {
            Ok(window) => window,
            Err(error) => {
                eprintln!("the fixture window could not open: {error:#}");
                cx.quit();
                return;
            }
        };
        // The manifest is written after the first frame, from the view's
        // own state and the window's own measurements; the capture helper
        // waits for the file before it captures anything.
        let written = window.update(cx, |_, window, cx| {
            let path = manifest_path.clone();
            window.on_next_frame(move |window, cx| {
                let result = window
                    .root::<FixtureWindow>()
                    .flatten()
                    .ok_or_else(|| "the fixture window has no fixture view".to_owned())
                    .and_then(|fixture| write_json(&path, &manifest(fixture.read(cx), window, cx)));
                if let Err(error) = result {
                    eprintln!("{error}");
                    cx.quit();
                }
            });
            cx.notify();
        });
        if let Err(error) = written {
            eprintln!("the fixture window closed before its first frame: {error:#}");
            cx.quit();
            return;
        }
        let fixture_window = window.window_id();
        cx.on_window_closed(move |cx, closed| {
            if closed == fixture_window {
                cx.quit();
            }
        })
        .detach();
    });
    Ok(())
}

// ------------------------------------------------- the clipboard split view

/// What a fixture clip's row leads with: a tile, or a color's swatch
/// (`0xRRGGBBAA`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FixtureMark {
    Tile(ClipTone, Glyph),
    Swatch(u32),
}

/// Which preview a fixture clip opens: the reference template's branches.
/// Production previews every record as plain text; the others are the
/// reference fixture's, drawn only here (#100).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FixturePreview {
    Code,
    Text,
    Color {
        fill: u32,
        hex: &'static str,
        values: [&'static str; 2],
    },
    Link {
        domain: &'static str,
        url: &'static str,
    },
    Image {
        dims: &'static str,
    },
}

impl FixturePreview {
    /// The template branch's name, as the reference's state names it.
    fn kind(self) -> &'static str {
        match self {
            FixturePreview::Code => "code",
            FixturePreview::Text => "text",
            FixturePreview::Color { .. } => "color",
            FixturePreview::Link { .. } => "link",
            FixturePreview::Image { .. } => "image",
        }
    }
}

/// One clip of the fixture: the reference board's `all[]`, or a
/// production record's presentation values.
#[derive(Debug, PartialEq)]
pub(crate) struct FixtureClip {
    id: &'static str,
    /// The section it is listed under.
    group: &'static str,
    /// Its kind as a tab keeps it: "Text", "Link", "Image" or "Color".
    kind: &'static str,
    title: &'static str,
    body: &'static str,
    /// The source application, as the copied line names it ("" for none).
    app: &'static str,
    /// The row's time.
    time: &'static str,
    /// When it was copied, as the copied line says it ("Today, 14:02").
    when: &'static str,
    mark: FixtureMark,
    preview: FixturePreview,
}

impl FixtureClip {
    /// The footer's line for this clip, by the reference's `copiedLine`
    /// (which production's `copied_line` words the same way).
    fn copied(&self) -> String {
        let when = match self.when.split_once(", ") {
            Some((day @ ("Today" | "Yesterday"), time)) => {
                format!("{}, {time}", day.to_lowercase())
            }
            _ => format!("on {}", self.when),
        };
        if self.app.is_empty() {
            format!("Copied {when}")
        } else {
            format!("Copied {when} from {}", self.app)
        }
    }
}

const CODE: &str =
    "const pane = createPane({\n  blur: 44,\n  tint: 0.7,\n  accent: \"#C9EE6A\",\n})";

/// The reference clipboard board's clips, in its order (`clipboard.js`).
pub(crate) const REFERENCE_CLIPS: &[FixtureClip] = &[
    FixtureClip {
        id: "ssh",
        group: "Pinned",
        kind: "Text",
        title: "ssh deploy@10.0.4.12",
        body: "ssh deploy@10.0.4.12",
        app: "Terminal",
        time: "Mon",
        when: "Monday, 09:12",
        mark: FixtureMark::Tile(ClipTone::Term, Glyph::Terminal),
        preview: FixturePreview::Text,
    },
    FixtureClip {
        id: "code",
        group: "Today",
        kind: "Text",
        title: "const pane = createPane({",
        body: CODE,
        app: "Visual Studio Code",
        time: "14:02",
        when: "Today, 14:02",
        mark: FixtureMark::Tile(ClipTone::Code, Glyph::Code),
        preview: FixturePreview::Code,
    },
    FixtureClip {
        id: "lime",
        group: "Today",
        kind: "Color",
        title: "#C9EE6A",
        body: "",
        app: "Color Picker",
        time: "13:48",
        when: "Today, 13:48",
        mark: FixtureMark::Swatch(0xC9EE6AFF),
        preview: FixturePreview::Color {
            fill: 0xC9EE6AFF,
            hex: "#C9EE6A",
            values: ["rgb(201 238 106)", "hsl(77 79% 67%)"],
        },
    },
    FixtureClip {
        id: "link",
        group: "Today",
        kind: "Link",
        title: "example.com/plugins/manifest",
        body: "",
        app: "Firefox",
        time: "13:31",
        when: "Today, 13:31",
        mark: FixtureMark::Tile(ClipTone::Web, Glyph::Link),
        preview: FixturePreview::Link {
            domain: "example.com",
            url: "https://example.com/plugins/manifest",
        },
    },
    FixtureClip {
        id: "shot",
        group: "Today",
        kind: "Image",
        title: "Screenshot 2880 × 1800",
        body: "",
        app: "Screenshot",
        time: "12:10",
        when: "Today, 12:10",
        mark: FixtureMark::Tile(ClipTone::Folder, Glyph::Image),
        preview: FixturePreview::Image {
            dims: "2880 × 1800",
        },
    },
    FixtureClip {
        id: "standup",
        group: "Today",
        kind: "Text",
        title: "Standup moved to 10:30 tomorrow",
        body: "Standup moved to 10:30 tomorrow — same room, bring the launcher demo.",
        app: "Slack",
        time: "11:04",
        when: "Today, 11:04",
        mark: FixtureMark::Tile(ClipTone::Chat, Glyph::Lines),
        preview: FixturePreview::Text,
    },
    FixtureClip {
        id: "mail",
        group: "Yesterday",
        kind: "Text",
        title: "hello@example.com",
        body: "hello@example.com",
        app: "Mail",
        time: "17:22",
        when: "Yesterday, 17:22",
        mark: FixtureMark::Tile(ClipTone::Plain, Glyph::Mail),
        preview: FixturePreview::Text,
    },
    FixtureClip {
        id: "ice",
        group: "Yesterday",
        kind: "Color",
        title: "#8FD3FF",
        body: "",
        app: "Figma",
        time: "16:40",
        when: "Yesterday, 16:40",
        mark: FixtureMark::Swatch(0x8FD3FFFF),
        preview: FixturePreview::Color {
            fill: 0x8FD3FFFF,
            hex: "#8FD3FF",
            values: ["rgb(143 211 255)", "hsl(204 100% 78%)"],
        },
    },
];

const RELEASE_NOTES: &str = "Release notes, draft\n\nThe launcher now shows clipboard history as a list beside a preview. \
Search finds text you copied and the program you copied it from; Enter copies a record again, and Ctrl+D \
deletes it.\n\nNothing about what is kept changed: history stays off until you turn it on, a copy an \
application marks as private is skipped, programs you exclude are never read, and every record is \
deleted once it is older than the time you keep them for.\n\nThe list keeps the newest first, under \
Today, Yesterday and Older, in your own time zone. A long record like this one scrolls in its preview \
rather than being cut short, so the whole of what you copied can always be read before you copy it \
again.";

/// Production's own content, for the native-only scenarios: text
/// records, each previewed as text, under Today, Yesterday and Older, with
/// the source program Windows names (or none).
pub(crate) const PRODUCTION_CLIPS: &[FixtureClip] = &[
    FixtureClip {
        id: "9",
        group: "Today",
        kind: "Text",
        title: "git commit -m \"Port the split view\"",
        body: "git commit -m \"Port the split view\"",
        app: "WindowsTerminal.exe",
        time: "14:02",
        when: "Today, 14:02",
        mark: FixtureMark::Tile(ClipTone::Plain, Glyph::Lines),
        preview: FixturePreview::Text,
    },
    FixtureClip {
        id: "8",
        group: "Today",
        kind: "Text",
        title: "Standup moved to 10:30 tomorrow — same room",
        body: "Standup moved to 10:30 tomorrow — same room, bring the launcher demo.",
        app: "Slack.exe",
        time: "11:04",
        when: "Today, 11:04",
        mark: FixtureMark::Tile(ClipTone::Plain, Glyph::Lines),
        preview: FixturePreview::Text,
    },
    FixtureClip {
        id: "7",
        group: "Yesterday",
        kind: "Text",
        title: "hello@example.com",
        body: "hello@example.com",
        app: "OUTLOOK.EXE",
        time: "17:22",
        when: "Yesterday, 17:22",
        mark: FixtureMark::Tile(ClipTone::Plain, Glyph::Lines),
        preview: FixturePreview::Text,
    },
    FixtureClip {
        id: "5",
        group: "Older",
        kind: "Text",
        title: "Release notes, draft",
        body: RELEASE_NOTES,
        app: "notepad.exe",
        time: "Thu",
        when: "Thursday, 09:00",
        mark: FixtureMark::Tile(ClipTone::Plain, Glyph::Lines),
        preview: FixturePreview::Text,
    },
    FixtureClip {
        id: "4",
        group: "Older",
        kind: "Text",
        title: "ssh deploy@10.0.4.12",
        body: "ssh deploy@10.0.4.12",
        app: "",
        time: "Sep 28",
        when: "Sep 28, 16:12",
        mark: FixtureMark::Tile(ClipTone::Plain, Glyph::Lines),
        preview: FixturePreview::Text,
    },
];

/// Whose content a clipboard scenario shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipVariant {
    /// The reference board's own clips, labels and buttons, for parity.
    Reference,
    /// Production's: text records, All and Text, Copy, Delete and Manage,
    /// history on.
    Production,
    /// Production's with nothing kept and history off.
    Off,
}

impl ClipVariant {
    fn of(scenario: &Scenario) -> ClipVariant {
        match scenario.name {
            "clipboard-off" => ClipVariant::Off,
            "clipboard-production" | "clipboard-narrow" => ClipVariant::Production,
            _ => ClipVariant::Reference,
        }
    }

    fn name(self) -> &'static str {
        match self {
            ClipVariant::Reference => "reference",
            ClipVariant::Production => "production",
            ClipVariant::Off => "off",
        }
    }

    fn clips(self) -> &'static [FixtureClip] {
        match self {
            ClipVariant::Reference => REFERENCE_CLIPS,
            ClipVariant::Production => PRODUCTION_CLIPS,
            ClipVariant::Off => &[],
        }
    }

    /// Production's capture state for the variant.
    fn capture(self) -> CaptureState {
        match self {
            ClipVariant::Off => CaptureState::Off,
            ClipVariant::Reference | ClipVariant::Production => CaptureState::On,
        }
    }

    /// The command's chip.
    fn chip(self) -> &'static str {
        match self {
            ClipVariant::Reference => "Clipboard",
            ClipVariant::Production | ClipVariant::Off => "Clipboard History",
        }
    }

    fn placeholder(self) -> String {
        match self {
            ClipVariant::Reference => format!("Search {} clips…", REFERENCE_CLIPS.len()),
            ClipVariant::Production | ClipVariant::Off => {
                clipboard_history::placeholder(self.clips().len())
            }
        }
    }

    /// The capture button: its label, glyph and whether it shows pressed.
    fn capture_button(self) -> (&'static str, Glyph, bool) {
        match self {
            // The reference's Pause, not pressed.
            ClipVariant::Reference => ("Pause", Glyph::Pause, false),
            ClipVariant::Production | ClipVariant::Off => {
                clipboard_history::capture_control(self.capture())
            }
        }
    }

    /// The tabs' labels.
    fn tabs(self) -> Vec<&'static str> {
        match self {
            ClipVariant::Reference => vec!["All", "Text", "Links", "Images", "Colors"],
            ClipVariant::Production | ClipVariant::Off => {
                ClipboardFilter::ALL.map(ClipboardFilter::label).to_vec()
            }
        }
    }

    /// The caption on the tab strip's right.
    fn caption(self) -> String {
        match self {
            ClipVariant::Reference => "Password managers are never recorded".into(),
            ClipVariant::Production | ClipVariant::Off => {
                capture_summary(self.capture(), None, 7 * 86_400, 0)
            }
        }
    }

    /// The note in place of rows, with clips there are (`kept`) or not.
    fn empty_note(self, kept: bool) -> String {
        match self {
            ClipVariant::Reference => "No clips match. Try another filter.".into(),
            ClipVariant::Production | ClipVariant::Off => {
                clipboard_history::empty_note(None, kept, self.capture())
            }
        }
    }

    /// The footer's buttons: (label, binding, cap style) for the primary,
    /// the secondary (both only with a clip selected) and the last.
    fn buttons(self) -> [(&'static str, &'static str, CapStyle); 3] {
        match self {
            ClipVariant::Reference => [
                ("Paste to Obsidian", "enter", CapStyle::Accent),
                ("Copy", "ctrl-c", CapStyle::Regular),
                ("Actions", "ctrl-k", CapStyle::Regular),
            ],
            ClipVariant::Production | ClipVariant::Off => [
                ("Copy", "enter", CapStyle::Accent),
                (
                    "Delete",
                    clipboard_history::DELETE_BINDING,
                    CapStyle::Regular,
                ),
                ("Manage", "ctrl-k", CapStyle::Regular),
            ],
        }
    }
}

/// The fixture's split view state: the reference's `state` (query, tab,
/// selected id), over the variant's clips, by the reference's own rules —
/// which production's [`pane_core::clipboard_view::ClipboardBrowse`]
/// follows too.
#[derive(Clone, Debug)]
pub(crate) struct ClipState {
    variant: ClipVariant,
    query: String,
    tab: &'static str,
    selected: &'static str,
}

impl ClipState {
    fn new(scenario: &Scenario) -> ClipState {
        let variant = ClipVariant::of(scenario);
        ClipState {
            variant,
            query: String::new(),
            tab: "All",
            // The reference selects its code clip; production the first.
            selected: match variant {
                ClipVariant::Reference => "code",
                ClipVariant::Production | ClipVariant::Off => {
                    variant.clips().first().map_or("", |clip| clip.id)
                }
            },
        }
    }

    /// The clips the tab and the query keep, in order.
    fn visible(&self) -> Vec<&'static FixtureClip> {
        let kind = match self.tab {
            "Links" => Some("Link"),
            "Images" => Some("Image"),
            "Colors" => Some("Color"),
            "Text" if self.variant == ClipVariant::Reference => Some("Text"),
            _ => None,
        };
        let needle = self.query.trim().to_lowercase();
        self.variant
            .clips()
            .iter()
            .filter(|clip| kind.is_none_or(|kind| clip.kind == kind))
            .filter(|clip| {
                needle.is_empty()
                    || format!("{} {} {}", clip.title, clip.body, clip.app)
                        .to_lowercase()
                        .contains(&needle)
            })
            .collect()
    }

    /// The selected clip's index among `visible`: the chosen one while it
    /// shows, else the first; none with none shown.
    fn selected_index(&self, visible: &[&FixtureClip]) -> Option<usize> {
        if visible.is_empty() {
            return None;
        }
        Some(
            visible
                .iter()
                .position(|clip| clip.id == self.selected)
                .unwrap_or(0),
        )
    }

    /// The section labels over `visible`: one per run of a group.
    fn sections(visible: &[&FixtureClip]) -> Vec<SectionLabel> {
        let mut labels: Vec<SectionLabel> = Vec::new();
        for (index, clip) in visible.iter().enumerate() {
            if labels.last().is_none_or(|label| label.label != clip.group) {
                labels.push(SectionLabel {
                    first: index,
                    label: clip.group.into(),
                    note: None,
                });
            }
        }
        labels
    }

    /// Up or Down: within what is shown, stopping at its ends.
    fn step(&mut self, delta: isize) {
        let visible = self.visible();
        if let Some(index) = self.selected_index(&visible) {
            let next = index.saturating_add_signed(delta).min(visible.len() - 1);
            self.selected = visible[next].id;
        }
    }

    fn set_query(&mut self, query: &str) {
        self.query = query.to_owned();
    }

    /// A click on `target`: a tab ("clip-tab-text") or a clip
    /// ("clip-standup").
    fn click(&mut self, target: &str) {
        let tabs = self.variant.tabs();
        if let Some(tab) = target.strip_prefix("clip-tab-")
            && let Some(&label) = tabs.iter().find(|label| label.to_lowercase() == tab)
        {
            self.tab = label;
        } else if let Some(id) = target.strip_prefix("clip-")
            && let Some(clip) = self.variant.clips().iter().find(|clip| clip.id == id)
        {
            self.selected = clip.id;
        }
    }
}

/// The split view's frame in a client of `client` size, as
/// [`split_view::compose`] lays it out: the header, the tab strip, the
/// list beside the preview pane, the card inside it, and the footer.
#[derive(Clone, Copy, Debug)]
struct ClipFrame {
    header: Rect,
    tabs: Rect,
    list: Rect,
    pane: Rect,
    card: Rect,
    footer: Rect,
}

fn clip_frame(theme: &Theme, client: (f32, f32)) -> ClipFrame {
    let split = &theme.split;
    let f = f32::from;
    let (width, height) = client;
    let header = Rect {
        x: 0.,
        y: 0.,
        width,
        height: f(split.header_height),
    };
    let tabs = Rect {
        x: 0.,
        y: header.height,
        width,
        height: f(split.tabs_height),
    };
    let footer = Rect {
        x: 0.,
        y: height - f(split.footer_height),
        width,
        height: f(split.footer_height),
    };
    let top = tabs.y + tabs.height;
    let list_width = f(split.list_width).min(width * split.list_max_share);
    let list = Rect {
        x: 0.,
        y: top,
        width: list_width,
        height: footer.y - top,
    };
    let pane = Rect {
        x: list_width,
        y: top,
        width: width - list_width,
        height: list.height,
    };
    let padding = f(split.preview_padding);
    let card = Rect {
        x: pane.x + padding,
        y: pane.y + padding,
        width: pane.width - 2. * padding,
        height: pane.height - 2. * padding,
    };
    ClipFrame {
        header,
        tabs,
        list,
        pane,
        card,
        footer,
    }
}

/// A clip's row as a capture declares it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredClip {
    id: &'static str,
    title: &'static str,
    time: &'static str,
    rect: Rect,
    selected: bool,
    hovered: bool,
    visible: bool,
    /// "tile" or "swatch".
    mark: &'static str,
}

/// The split view as a capture declares it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredClipboard {
    variant: &'static str,
    header: Rect,
    tabs_strip: Rect,
    list: Rect,
    pane: Rect,
    footer: Rect,
    query: String,
    tab: &'static str,
    rows: Vec<DeclaredClip>,
    sections: Vec<DeclaredSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    empty: Option<DeclaredText>,
    /// The preview card and its branch, with a clip selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<DeclaredPreview>,
    /// The footer's copied line (or "Nothing selected").
    copied: DeclaredText,
    caption: String,
    /// The back button and the capture button's rect and label.
    back: Rect,
    capture: DeclaredText,
    /// The tabs, filled in from shaped labels when the manifest is
    /// written.
    tabs: Vec<DeclaredTab>,
    /// The footer's buttons, left to right, filled in likewise.
    buttons: Vec<DeclaredText>,
    /// The rule between the footer's buttons, with a clip selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    divider: Option<Rect>,
    /// The split view's own colors, as `#RRGGBBAA`.
    colors: ClipColors,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredPreview {
    kind: &'static str,
    rect: Rect,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeclaredTab {
    label: &'static str,
    on: bool,
    rect: Rect,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClipColors {
    back_fill: String,
    chip_fill: String,
    tab_on: String,
    tab_text: String,
    preview_fill: String,
    preview_edge: String,
}

/// The rows and labels `state` shows down the list of `frame`, scrolled
/// by `offset` (0 or negative), with the pointer at `pointer`.
fn declared_clips(
    state: &ClipState,
    theme: &Theme,
    frame: &ClipFrame,
    offset: f32,
    pointer: Option<(f32, f32)>,
) -> (Vec<DeclaredClip>, Vec<DeclaredSection>) {
    let split = &theme.split;
    let geometry = &theme.geometry;
    let f = f32::from;
    let visible = state.visible();
    let selected = state.selected_index(&visible);
    let sections = ClipState::sections(&visible);
    let x = frame.list.x + f(split.list_padding_x);
    // The list's rule takes its last pixel.
    let width = frame.list.width - 2. * f(split.list_padding_x) - 1.;
    let mut y = frame.list.y + f(split.list_padding_top) + offset;
    let gap = f(geometry.row_list_gap);
    let list = frame.list;
    let mut labels = Vec::new();
    let mut rows = Vec::new();
    for (index, clip) in visible.iter().enumerate() {
        for section in sections.iter().filter(|section| section.first == index) {
            labels.push(DeclaredSection {
                label: section.label.to_string(),
                note: None,
                rect: Rect {
                    x,
                    y,
                    width,
                    height: f(geometry.section_height),
                },
            });
            y += f(geometry.section_height) + gap;
        }
        let rect = Rect {
            x,
            y,
            width,
            height: f(geometry.row_min_height),
        };
        y += rect.height + gap;
        rows.push(DeclaredClip {
            id: clip.id,
            title: clip.title,
            time: clip.time,
            rect,
            selected: selected == Some(index),
            hovered: pointer.is_some_and(|point| rect.contains(point)),
            visible: rect.y >= list.y && rect.y + rect.height <= list.y + list.height,
            mark: match clip.mark {
                FixtureMark::Tile(..) => "tile",
                FixtureMark::Swatch(_) => "swatch",
            },
        });
    }
    (rows, labels)
}

/// The list's offset after the keys move the selection, by the rule GPUI's
/// `ScrollHandle::scroll_to_item` applies (the least scroll that shows
/// the whole row), as the launcher's split view asks for it.
fn clip_scrolled(state: &ClipState, theme: &Theme, frame: &ClipFrame, offset: f32) -> f32 {
    let (rows, _) = declared_clips(state, theme, frame, 0., None);
    let Some(row) = rows.iter().find(|row| row.selected) else {
        return offset;
    };
    let (top, bottom) = (frame.list.y, frame.list.y + frame.list.height);
    if row.rect.y + offset < top {
        top - row.rect.y
    } else if row.rect.y + row.rect.height + offset > bottom {
        bottom - (row.rect.y + row.rect.height)
    } else {
        offset
    }
}

/// Replays a clipboard scenario's steps over the split view's state:
/// declares each capture and resolves each click on a clip.
fn replay_clipboard(scenario: &Scenario, theme: &Theme) -> Replay {
    let frame = clip_frame(theme, scenario.client);
    let split = &theme.split;
    let f = f32::from;
    let mut state = ClipState::new(scenario);
    let mut offset = 0.;
    let mut pointer = None;
    let mut after = Vec::new();
    let mut captures = Vec::new();
    let mut steps = Vec::new();
    for step in scenario.steps {
        let mut point = None;
        match *step {
            Step::Capture { name } => {
                let (rows, sections) = declared_clips(&state, theme, &frame, offset, pointer);
                let visible = state.visible();
                let selected = state.selected_index(&visible).map(|index| visible[index]);
                let empty = visible.is_empty().then(|| DeclaredText {
                    text: state.variant.empty_note(!state.variant.clips().is_empty()),
                    rect: Rect {
                        x: frame.list.x + f(split.list_padding_x),
                        y: frame.list.y + f(split.list_padding_top),
                        width: frame.list.width - 2. * f(split.list_padding_x) - 1.,
                        height: 2. * f(split.empty_padding_y)
                            + f(split.empty_size) * theme.typography.line_height,
                    },
                });
                let lead = f(theme.geometry.footer_padding_left)
                    + f(split.footer_glyph)
                    + f(split.footer_lead_gap);
                let button = f(theme.geometry.action_height);
                captures.push(DeclaredCapture {
                    name,
                    after: after.clone(),
                    query: state.query.clone(),
                    pointer,
                    rows: Vec::new(),
                    sections: Vec::new(),
                    action: None,
                    action_button: None,
                    actions_open: false,
                    actions: None,
                    footer: None,
                    board: None,
                    clipboard: Some(DeclaredClipboard {
                        variant: state.variant.name(),
                        header: frame.header,
                        tabs_strip: frame.tabs,
                        list: frame.list,
                        pane: frame.pane,
                        footer: frame.footer,
                        query: state.query.clone(),
                        tab: state.tab,
                        rows,
                        sections,
                        empty,
                        preview: selected.map(|clip| DeclaredPreview {
                            kind: clip.preview.kind(),
                            rect: frame.card,
                        }),
                        copied: DeclaredText {
                            text: selected.map_or_else(
                                || "Nothing selected".to_owned(),
                                |clip| clip.copied(),
                            ),
                            rect: Rect {
                                x: lead,
                                y: frame.footer.y,
                                width: 0.,
                                height: frame.footer.height,
                            },
                        },
                        caption: state.variant.caption(),
                        back: Rect {
                            x: f(split.header_padding_left),
                            y: (frame.header.height - 1. - f(split.back_size)) / 2.,
                            width: f(split.back_size),
                            height: f(split.back_size),
                        },
                        capture: DeclaredText {
                            text: state.variant.capture_button().0.to_owned(),
                            rect: Rect {
                                x: 0.,
                                y: (frame.header.height - 1. - button) / 2.,
                                width: 0.,
                                height: button,
                            },
                        },
                        tabs: Vec::new(),
                        buttons: Vec::new(),
                        divider: None,
                        colors: ClipColors {
                            back_fill: hex(split.back_fill),
                            chip_fill: hex(split.chip_fill),
                            tab_on: hex(split.tab_on),
                            tab_text: hex(split.tab_text),
                            preview_fill: hex(split.preview_fill),
                            preview_edge: hex(split.preview_edge),
                        },
                    }),
                    pinned: None,
                });
            }
            Step::Key {
                key: NamedKey::Down,
            } => state.step(1),
            Step::Key {
                key: NamedKey::Escape,
            } => state.set_query(""),
            Step::Type { text } => {
                let query = format!("{}{text}", state.query);
                state.set_query(&query);
            }
            Step::Click { target } => {
                // A clip's click lands at its row's center; a tab's is
                // placed once its label is shaped (the manifest fills it).
                if !target.starts_with("clip-tab-") {
                    let (rows, _) = declared_clips(&state, theme, &frame, offset, None);
                    let id = target.strip_prefix("clip-").unwrap_or(target);
                    point = rows
                        .iter()
                        .find(|row| row.id == id)
                        .map(|row| row.rect.center());
                }
                state.click(target);
                if point.is_some() {
                    pointer = point;
                }
            }
            // The clipboard scenarios move the pointer only by clicking.
            Step::Pointer { .. } | Step::Point { .. } => {}
        }
        if matches!(step, Step::Key { .. }) {
            offset = clip_scrolled(&state, theme, &frame, offset);
        }
        if !matches!(step, Step::Capture { .. }) {
            after.push(*step);
        }
        steps.push(ResolvedStep { step: *step, point });
    }
    Replay { captures, steps }
}

/// The parts of the split view only shaping places, for a clipboard
/// scenario's manifest: the chip, the capture button, the tabs and the
/// footer's buttons; and the clicks on a tab.
fn declare_clipboard(
    window: &Window,
    theme: &Theme,
    keyboard: &pane_core::Keyboard,
    captures: &mut [DeclaredCapture],
    steps: &mut [ResolvedStep],
) {
    let split = &theme.split;
    let geometry = &theme.geometry;
    let typography = &theme.typography;
    let f = f32::from;
    // Each label is a text box, which GPUI lays out at its shaped width
    // rounded up to a whole pixel: the tabs and the footer's buttons chain
    // several, so their places are declared at the laid-out widths.
    let label_width = |text: &str, size: Pixels| {
        shaped_width(window, theme, text, size, typography.medium).ceil()
    };
    for capture in captures.iter_mut() {
        let Some(clip) = capture.clipboard.as_mut() else {
            continue;
        };
        let variant = match clip.variant {
            "production" => ClipVariant::Production,
            "off" => ClipVariant::Off,
            _ => ClipVariant::Reference,
        };
        // The tabs, from the strip's left padding, 4 apart.
        let mut x = f(split.tabs_padding_x);
        let top = clip.tabs_strip.y + (clip.tabs_strip.height - 1. - f(split.tab_height)) / 2.;
        clip.tabs = variant
            .tabs()
            .into_iter()
            .map(|label| {
                let width = 2. * f(split.tab_padding_x) + label_width(label, split.tab_size);
                let tab = DeclaredTab {
                    label,
                    on: label == clip.tab,
                    rect: Rect {
                        x,
                        y: top,
                        width,
                        height: f(split.tab_height),
                    },
                };
                x += width + f(split.tabs_gap);
                tab
            })
            .collect();
        // The capture button, at the header's right padding.
        let (capture_label, _, _) = variant.capture_button();
        let capture_width = 2. * f(geometry.action_padding_x)
            + f(split.capture_glyph)
            + f(geometry.action_gap)
            + label_width(capture_label, typography.footer_size);
        clip.capture.rect.x = clip.header.width - f(split.header_padding_right) - capture_width;
        clip.capture.rect.width = capture_width;
        // The footer's buttons, right to left from its right padding.
        let selected = clip.preview.is_some();
        let button_top =
            clip.footer.y + 1. + (clip.footer.height - 1. - f(geometry.action_height)) / 2.;
        let mut right = clip.footer.width - f(geometry.footer_padding_right);
        let mut buttons = Vec::new();
        let [primary, secondary, more] = variant.buttons();
        let shown: Vec<_> = if selected {
            vec![Some(more), None, Some(secondary), Some(primary)]
        } else {
            vec![Some(more)]
        };
        for entry in shown {
            match entry {
                Some((label, binding, style)) => {
                    let keys = crate::keyboard::binding_keys(&effective(keyboard, binding));
                    let width = 2. * f(geometry.action_padding_x)
                        + label_width(label, typography.footer_size)
                        + f(geometry.action_gap)
                        + laid_keys_width(window, theme, &keys, style);
                    buttons.push(DeclaredText {
                        text: label.to_owned(),
                        rect: Rect {
                            x: right - width,
                            y: button_top,
                            width,
                            height: f(geometry.action_height),
                        },
                    });
                    right -= width + f(geometry.footer_buttons_gap);
                }
                // The rule between the selected clip's buttons and the last.
                None => {
                    let height = f(geometry.footer_divider_height);
                    clip.divider = Some(Rect {
                        x: right - 1.,
                        y: clip.footer.y + 1. + (clip.footer.height - 1. - height) / 2.,
                        width: 1.,
                        height,
                    });
                    right -= 1. + f(geometry.footer_buttons_gap);
                }
            }
        }
        buttons.reverse();
        clip.buttons = buttons;
    }
    // A click on a tab lands at its center.
    let Some(first) = captures
        .iter()
        .find_map(|capture| capture.clipboard.as_ref())
    else {
        return;
    };
    let tabs = first.tabs.clone();
    for step in steps.iter_mut() {
        if let Step::Click { target } = step.step
            && let Some(name) = target.strip_prefix("clip-tab-")
            && let Some(tab) = tabs.iter().find(|tab| tab.label.to_lowercase() == name)
        {
            step.point = Some(tab.rect.center());
        }
    }
}

/// The binding a fixture button shows: the effective one for the keys the
/// launcher rebinds (Enter, Ctrl+K), else the fixed one it names.
fn effective(keyboard: &pane_core::Keyboard, binding: &str) -> Binding {
    match binding {
        "enter" => keyboard
            .binding(KeyboardAction::InvokeSelectedAction)
            .clone(),
        "ctrl-k" => keyboard.binding(KeyboardAction::OpenActions).clone(),
        other => parse_binding(other),
    }
}

impl FixtureWindow {
    fn clip_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.clip_step(1, cx);
    }

    fn clip_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.clip_step(-1, cx);
    }

    /// The keys' selection, kept in view as the launcher's split view
    /// keeps it.
    fn clip_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(clip) = self.clip.as_mut() else {
            return;
        };
        clip.step(delta);
        let visible = clip.visible();
        if let Some(selected) = clip.selected_index(&visible) {
            let labels = ClipState::sections(&visible);
            self.scroll
                .scroll_to_item(shell::child_of_row(&labels, selected));
        }
        cx.notify();
    }

    /// Escape: the reference's clears the query.
    fn clip_back(&mut self, _: &Back, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(clip) = self.clip.as_mut() {
            clip.set_query("");
        }
        let query = self.query.clone();
        query.update(cx, |query, cx| query.emplace("", cx));
        cx.notify();
    }

    /// The split view over the scenario's clips, composed by the
    /// production parts the launcher's Clipboard History composes
    /// (`crate::ui::split_view`, `crate::ui::footer`).
    fn render_clipboard(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let Some(clip) = self.clip.as_ref() else {
            return div();
        };
        let variant = clip.variant;
        let visible = clip.visible();
        let selected = clip.selected_index(&visible);
        let labels = ClipState::sections(&visible);
        let (capture_label, capture_glyph, pressed) = variant.capture_button();
        let header = split_view::header(
            split_view::back_button(theme).into_any_element(),
            split_view::chip(variant.chip(), theme),
            split_view::search_field(&self.query, variant.placeholder(), theme).into_any_element(),
            Some(
                split_view::capture_button(capture_label, capture_glyph, pressed, theme)
                    .into_any_element(),
            ),
            theme,
        );
        let tabs = variant
            .tabs()
            .into_iter()
            .map(|label| {
                let target = format!("clip-tab-{}", label.to_lowercase());
                split_view::tab(
                    SharedString::from(target.clone()),
                    label,
                    label == clip.tab,
                    theme,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(clip) = this.clip.as_mut() {
                        clip.click(&target);
                        cx.notify();
                    }
                }))
                .into_any_element()
            })
            .collect();
        let tabs = split_view::tabs(
            tabs,
            Some((variant.caption().into(), theme.text_muted)),
            theme,
        );
        let rows: Vec<gpui::AnyElement> = visible
            .iter()
            .enumerate()
            .map(|(index, record)| {
                let target = format!("clip-{}", record.id);
                split_view::clip_row(
                    ("clip", index),
                    ClipRow {
                        title: record.title.into(),
                        time: record.time.into(),
                        selected: selected == Some(index),
                        mark: match record.mark {
                            FixtureMark::Tile(tone, glyph) => ClipMark::Tile(tone, glyph),
                            FixtureMark::Swatch(color) => {
                                ClipMark::Swatch(gpui::rgb_to_hsla(gpui::rgba(color)))
                            }
                        },
                    },
                    theme,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(clip) = this.clip.as_mut() {
                        clip.click(&target);
                        cx.notify();
                    }
                }))
                .into_any_element()
            })
            .collect();
        let list = split_view::list(theme).track_scroll(&self.scroll);
        let list = if visible.is_empty() {
            let note = variant.empty_note(!variant.clips().is_empty());
            list.child(split_view::empty_note(note, theme))
        } else {
            list.children(shell::with_section_labels(rows, &labels, theme))
        };
        let preview = selected.map(|index| {
            let record = visible[index];
            let content = match record.preview {
                FixturePreview::Code => {
                    let split = &theme.split;
                    let lines = code_lines(split);
                    let lines: Vec<&[(&str, Option<gpui::Hsla>)]> =
                        lines.iter().map(Vec::as_slice).collect();
                    split_view::code_preview(&lines, theme)
                }
                FixturePreview::Text => split_view::text_preview(record.body, theme),
                FixturePreview::Color { fill, hex, values } => split_view::color_preview(
                    gpui::rgb_to_hsla(gpui::rgba(fill)),
                    hex,
                    &values,
                    theme,
                ),
                FixturePreview::Link { domain, url } => {
                    split_view::link_preview(domain, url, theme)
                }
                FixturePreview::Image { dims } => {
                    split_view::image_preview("[Screenshot preview]", dims, theme)
                }
            };
            split_view::preview_card(
                SharedString::from(format!("clipboard-preview-{}", record.id)).into(),
                content.into_any_element(),
                theme,
            )
            .into_any_element()
        });
        let keyboard = settings::keyboard_of(cx);
        let [primary, secondary, more] = variant.buttons();
        let button = |(label, binding, style): (&'static str, &'static str, CapStyle),
                      id: &'static str| {
            let keys = crate::keyboard::binding_keys(&effective(&keyboard, binding));
            footer::footer_button(id, label, &keys, style, footer::ButtonWash::Hover, theme)
                .into_any_element()
        };
        let buttons = split_view::footer_buttons(
            selected.map(|_| button(primary, "clipboard-primary")),
            selected.map(|_| button(secondary, "clipboard-secondary")),
            button(more, "clipboard-more"),
            theme,
        );
        let copied = selected.map_or_else(
            || "Nothing selected".to_owned(),
            |index| visible[index].copied(),
        );
        let footer = split_view::footer(
            split_view::footer_lead(copied, theme.text_muted, theme),
            buttons,
            theme,
        );
        let content = split_view::compose(
            header,
            tabs,
            list.into_any_element(),
            preview,
            footer.into_any_element(),
            theme,
        )
        .key_context(clipboard_history::CONTEXT);
        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::clip_next))
            .on_action(cx.listener(Self::clip_previous))
            .on_action(cx.listener(Self::clip_back))
            .size_full()
            .flex()
            .flex_col()
            .font_family(theme.typography.family.clone())
            .font_features(theme.typography.features.clone())
            .text_color(theme.text_title)
            .child(content)
    }
}

/// The reference's default preview: `CODE`'s lines, each a run of spans
/// in the reference's keyword, function, number and string colors.
fn code_lines(
    split: &crate::ui::theme::SplitTokens,
) -> Vec<Vec<(&'static str, Option<gpui::Hsla>)>> {
    vec![
        vec![
            ("const", Some(split.code_keyword)),
            (" pane = ", None),
            ("createPane", Some(split.code_function)),
            ("({", None),
        ],
        vec![
            ("  blur: ", None),
            ("44", Some(split.code_value)),
            (",", None),
        ],
        vec![
            ("  tint: ", None),
            ("0.7", Some(split.code_value)),
            (",", None),
        ],
        vec![
            ("  accent: ", None),
            ("\"#C9EE6A\"", Some(split.code_string)),
            (",", None),
        ],
        vec![("})", None)],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::dark()
    }

    fn declared_captures(scenario: &Scenario, theme: &Theme) -> Vec<DeclaredCapture> {
        replay(scenario, theme).captures
    }

    fn scenario(name: &str) -> &'static Scenario {
        find_scenario(name).expect("the scenario is registered")
    }

    fn capture<'a>(captures: &'a [DeclaredCapture], name: &str) -> &'a DeclaredCapture {
        captures
            .iter()
            .find(|capture| capture.name == name)
            .unwrap_or_else(|| panic!("no capture {name}"))
    }

    #[test]
    fn every_reference_scenario_renders_at_its_boards_client() {
        for scenario in scenarios().iter().filter(|scenario| scenario.reference) {
            // The Settings and clipboard boards are panels of their own
            // sizes (#97, #102).
            let board = match scenario.family {
                Family::Settings => (1120., 720.),
                Family::Clipboard => (940., 600.),
                _ => (760., 518.),
            };
            assert_eq!(scenario.client, board, "{}", scenario.name);
        }
        // The launcher and Settings windows open at the same clients.
        assert_eq!(ROOT_CLIENT, crate::ui::shell::LAUNCHER_CLIENT);
        assert_eq!(SETTINGS_CLIENT, settings_shell::SETTINGS_CLIENT);
    }

    #[test]
    fn the_settings_frame_lays_the_shell_out_where_the_board_measures_it() {
        // The reference board's own measured boxes (its DOM, relative to
        // its glass panel): the 48px titlebar, the 232px sidebar, the
        // search well at 10,60 211x34, the sections from 104 every 38px,
        // the page from 232 and its heading at 264,74; the preview column
        // at 688.
        let frame = settings_frame(&theme(), SETTINGS_CLIENT);
        let rect = |x, y, width, height| Rect {
            x,
            y,
            width,
            height,
        };
        assert_eq!(frame.titlebar, rect(0., 0., 1120., 48.));
        assert_eq!(frame.sidebar, rect(0., 48., 232., 672.));
        assert_eq!(frame.search, rect(10., 60., 211., 34.));
        assert_eq!(frame.sections.len(), SETTINGS_SECTIONS.len());
        assert_eq!(frame.sections[0], rect(10., 104., 211., 36.));
        assert_eq!(frame.sections[1], rect(10., 142., 211., 36.));
        assert_eq!(frame.sections[7].y, 370.);
        assert_eq!(frame.page, rect(232., 48., 888., 672.));
        assert_eq!((frame.heading.x, frame.heading.y), (264., 74.));
        assert_eq!(frame.heading.width, 388.);
        // The heading's line is the board's 28 (#98), so its subtitle
        // begins at the board's 106.
        assert_eq!(frame.heading.height, 28.);
        assert_eq!(frame.subtitle_top, 106.);
        let aside = frame.aside.expect("the columns sit side by side");
        assert_eq!((aside.x, aside.y, aside.width), (688., 74., 400.));
        assert_eq!(aside.height, 16., "the caption's line");

        // A narrower window keeps the titlebar and the sidebar, and the
        // page's columns collapse into one.
        let narrow = settings_frame(&theme(), SETTINGS_NARROW_CLIENT);
        assert_eq!(narrow.sidebar, rect(0., 48., 232., 472.));
        assert_eq!(narrow.sections[0], frame.sections[0]);
        assert_eq!(narrow.heading.width, 760. - 232. - 64.);
        assert!(narrow.aside.is_none());
    }

    #[test]
    fn the_settings_scenario_points_at_its_sections_and_shows_the_boards_data() {
        let scenario = scenario("settings-shell");
        assert_eq!(scenario.board, Some("settings"));
        assert_eq!(scenario.family, Family::Settings);
        let labels: Vec<_> = SETTINGS_SECTIONS
            .iter()
            .map(|section| section.label)
            .collect();
        assert_eq!(
            labels,
            [
                "General",
                "Appearance",
                "Hotkeys & Aliases",
                "Plugins",
                "Window Manager",
                "Clipboard",
                "Privacy",
                "About"
            ]
        );
        assert_eq!(SETTINGS_SECTIONS[SETTINGS_SELECTED].label, "Appearance");
        let rows: Vec<_> = scenario
            .steps
            .iter()
            .filter_map(|step| match step {
                Step::Pointer { row, .. } => Some(*row),
                _ => None,
            })
            .collect();
        assert_eq!(rows, [0, SETTINGS_SELECTED]);
        // Neither the shell nor its Appearance page is pending (#98).
        assert!(pending_scenarios().is_empty());
        assert_eq!(appearance_page(scenario.name), None, "the shell alone");
    }

    #[test]
    fn the_appearance_scenarios_render_the_board_and_productions_page() {
        // The board's own page, reference-backed, on the Settings board.
        for name in ["appearance-page", "appearance-solid"] {
            let scenario = scenario(name);
            assert_eq!(appearance_page(name), Some(AppearancePage::Board));
            assert_eq!(
                (scenario.family, scenario.board),
                (Family::Settings, Some("settings"))
            );
            assert!(scenario.reference && scenario.client == SETTINGS_CLIENT);
        }
        // Production's page: native-only, the board shows no theme choice.
        for name in [
            "appearance-production",
            "appearance-light",
            "appearance-override",
            "appearance-narrow",
        ] {
            let scenario = scenario(name);
            assert!(matches!(
                appearance_page(name),
                Some(AppearancePage::Production { .. })
            ));
            assert!(!scenario.reference, "{name}");
        }
        assert_eq!(
            appearance_page("appearance-override"),
            Some(AppearancePage::Production { overridden: true })
        );
        assert_eq!(scenario("appearance-light").theme, Some("light"));
        assert_eq!(scenario("appearance-narrow").client, SETTINGS_NARROW_CLIENT);
        // The hover lands on Frost and the click on Solid, by the targets
        // the segments carry.
        assert_eq!(segment_target("Frost"), SEGMENT_FROST);
        assert_eq!(segment_target("Solid"), SEGMENT_SOLID);
        assert!(
            scenario("appearance-page")
                .steps
                .contains(&point(SEGMENT_FROST))
        );
        assert!(
            scenario("appearance-solid")
                .steps
                .contains(&click(SEGMENT_SOLID))
        );
        assert_eq!(BOARD_MATERIALS[BOARD_SOLID].0, "Solid");
    }

    #[test]
    fn panes_own_pages_are_native_only_scenarios_through_their_compositions() {
        // Pane's other Settings pages and its form (#99): derived
        // compositions, so no board pairs with them.
        let registered = [
            ("settings-general", Family::Settings),
            ("settings-general-recording", Family::Settings),
            ("settings-launcher", Family::Settings),
            ("settings-keyboard", Family::Settings),
            ("settings-extensions", Family::Settings),
            ("settings-extensions-confirm", Family::Settings),
            ("settings-about", Family::Settings),
            ("form-validation", Family::Form),
        ];
        for (name, family) in registered {
            let scenario = scenario(name);
            assert!(pages::pane_page(name).is_some(), "{name} draws a page");
            assert_eq!(scenario.family, family, "{name}");
            assert!(!scenario.reference && scenario.board.is_none(), "{name}");
            assert_eq!(appearance_page(name), None, "{name}");
        }
        assert_eq!(scenario("form-validation").client, ROOT_CLIENT);
        assert_eq!(scenario("settings-about").client, SETTINGS_CLIENT);
        // The select opens by a click on its trigger.
        assert!(
            scenario("settings-launcher")
                .steps
                .contains(&click(pages::SELECT_TRIGGER))
        );
        // The board's sidebar selects the section nearest each page.
        let nearest = |name| {
            let page = pages::pane_page(name);
            SETTINGS_SECTIONS[pages::selected_section(page, SETTINGS_SELECTED)].label
        };
        assert_eq!(nearest("settings-general"), "General");
        assert_eq!(nearest("settings-keyboard"), "Hotkeys & Aliases");
        assert_eq!(nearest("settings-extensions"), "Plugins");
        assert_eq!(nearest("settings-about"), "About");
        assert_eq!(nearest("settings-shell"), "Appearance");
    }

    #[test]
    fn the_appearance_board_data_is_the_boards() {
        let materials: Vec<_> = BOARD_MATERIALS.iter().map(|(name, _)| *name).collect();
        assert_eq!(materials, ["Glass", "Frost", "Solid"]);
        let swatches: Vec<_> = BOARD_SWATCHES.iter().map(|(name, _)| *name).collect();
        assert_eq!(swatches, ["Lime", "Ice", "Amber", "Lilac"]);
        assert_eq!(BOARD_SWATCHES[0].1, 0xC9EE6AFF, "Lime, the accent, chosen");
        assert_eq!(BOARD_DENSITIES[BOARD_DENSITY], "Default");
        assert_eq!(BOARD_TOGGLES.len(), 2);
        // Blur 44 of 0-60 and tint 70 of 55-95, as the board's ranges.
        assert_eq!(BOARD_BLUR.2, 44. / 60.);
        assert_eq!(BOARD_TINT.2, (70. - 55.) / (95. - 55.));
    }

    #[test]
    fn segments_share_their_track_as_the_board_lays_them_out() {
        // The board's Material track at 264,188 388x36: its segments at
        // 267, 395 and 523, 126x30 from 191.
        let track = Rect {
            x: 264.,
            y: 188.,
            width: 388.,
            height: 36.,
        };
        let segments = segment_rects(&theme(), track, 3);
        let xs: Vec<_> = segments.iter().map(|segment| segment.x).collect();
        assert_eq!(xs, [267., 395., 523.]);
        assert!(
            segments
                .iter()
                .all(|segment| (segment.y, segment.width, segment.height) == (191., 126., 30.))
        );
        // Two segments (production's material) share it in halves.
        let halves = segment_rects(&theme(), track, 2);
        assert_eq!((halves[0].width, halves[1].x), (190., 459.));
    }

    #[test]
    fn the_miniature_lies_where_the_board_measures_it() {
        // The board's DOM, relative to its glass panel: the caption at
        // 688,74, the stage at 688,100 400x520, the miniature at 718,156
        // 340x308 — its search line 46, its slots from 726,210 76.5 wide
        // and 82.5 apart, its rows from 262 every 40, its footer at 426.
        let frame = miniature_frame(&theme(), (688., 74.), 4, 4);
        let rect = |x, y, width, height| Rect {
            x,
            y,
            width,
            height,
        };
        assert_eq!(frame.caption, rect(688., 74., 400., 16.));
        assert_eq!(frame.stage, rect(688., 100., 400., 520.));
        assert_eq!(frame.panel, rect(718., 156., 340., 308.));
        assert_eq!(frame.search, rect(718., 156., 340., 46.));
        assert_eq!(frame.pins[0], rect(726., 210., 76.5, 44.));
        assert_eq!(frame.pins[3].x, 973.5);
        let tops: Vec<_> = frame.rows.iter().map(|row| row.y).collect();
        assert_eq!(tops, [262., 302., 342., 382.]);
        assert_eq!(frame.rows[0], rect(724., 262., 328., 38.));
        assert_eq!(frame.footer, rect(718., 426., 340., 38.));
    }

    #[test]
    fn the_segment_fault_is_a_named_perturbation() {
        assert_eq!(
            Perturbation::parse("segment-on-fill"),
            Ok(Some(Perturbation::SegmentOnFill))
        );
        let mut theme = theme();
        Perturbation::SegmentOnFill.apply(&mut theme);
        assert_ne!(theme.controls.segment_on, Theme::dark().controls.segment_on);
    }

    #[test]
    fn the_well_fault_is_a_named_perturbation() {
        assert_eq!(
            Perturbation::parse("well-fill"),
            Ok(Some(Perturbation::WellFill))
        );
        let mut theme = theme();
        Perturbation::WellFill.apply(&mut theme);
        assert_ne!(theme.field_fill, Theme::dark().field_fill);
    }

    #[test]
    fn the_root_frame_divides_the_client_as_the_reference_does() {
        // 64 search + 404 list + 50 footer = 518, edge to edge: the
        // panel's inset ring takes no layout space.
        let frame = frame(&theme(), ROOT_CLIENT);
        assert_eq!((frame.search.x, frame.search.y), (0., 0.));
        assert_eq!(frame.search.width, 760.);
        assert_eq!(frame.search.y + frame.search.height, frame.list.y);
        assert_eq!(frame.list.y + frame.list.height, frame.footer.y);
        assert_eq!(frame.footer.y + frame.footer.height, 518.);
        assert_eq!(frame.list.height, 404.);
    }

    #[test]
    fn light_and_narrow_frames_are_native_only_adaptations() {
        let light = scenario("launcher-frame-light");
        assert_eq!(light.theme, Some("light"));
        assert!(!light.reference && light.frame);
        let narrow = scenario("launcher-frame-narrow");
        assert_eq!(narrow.client, NARROW_CLIENT);
        assert!(!narrow.reference && narrow.frame);
        assert!(scenario("launcher-frame").reference);
    }

    #[test]
    fn the_narrow_list_scrolls_the_last_row_into_view_above_the_footer() {
        let captures = declared_captures(scenario("launcher-frame-narrow"), &theme());
        let rest = capture(&captures, "narrow-rest");
        // At rest the first rows show and the rest are below the fold.
        assert!(rest.rows[0].visible && rest.rows[0].selected);
        assert!(!rest.rows.last().unwrap().visible);
        let last = capture(&captures, "narrow-last-selected");
        let selected = last.rows.last().unwrap();
        assert!(selected.selected && selected.visible);
        // Scrolled by the least that shows it: its bottom on the list's.
        let list_bottom = NARROW_CLIENT.1 - 50.;
        assert_eq!(selected.rect.y + selected.rect.height, list_bottom);
        assert!(!last.rows[0].visible);
    }

    #[test]
    fn pending_scenarios_name_their_tickets_and_every_board_size_is_registered() {
        for pending in pending_scenarios() {
            assert!(
                pending
                    .ticket
                    .starts_with("https://github.com/hoangvu12/pane/issues/"),
                "{}",
                pending.name
            );
            assert!(
                find_scenario(pending.name)
                    .unwrap_err()
                    .contains(pending.ticket)
            );
        }
        // Every board size has registered reference scenarios: the root's
        // (the pinned home among them, #101), the Settings board's and the
        // clipboard board's (#102).
        for client in [ROOT_CLIENT, SETTINGS_CLIENT, CLIPBOARD_CLIENT] {
            assert!(
                scenarios()
                    .iter()
                    .any(|scenario| scenario.reference && scenario.client == client)
            );
        }
        // Nothing is pending: the Appearance page (#98) was the last, and
        // asking for it renders it.
        assert!(pending_scenarios().is_empty());
        assert!(scenario("appearance-page").reference);
        assert!(scenario("pinned-strip").reference);
        // Store and the snap HUD stay source-only references.
        assert!(
            pending_scenarios()
                .iter()
                .all(|p| p.board != "store" && p.board != "window-manager")
        );
    }

    #[test]
    fn the_root_rows_are_the_reference_boards_suggested_then_commands() {
        let titles: Vec<_> = ROOT_ROWS.iter().map(|row| row.title).collect();
        assert_eq!(
            titles,
            [
                "Figma",
                "Clipboard History",
                "Left Half",
                "Search Files",
                "Plugin Store",
                "Toggle Dark Mode",
                "Lock Screen",
                "Settings"
            ]
        );
    }

    #[test]
    fn rows_are_laid_out_down_the_list_from_its_top_padding() {
        let captures = declared_captures(scenario("root-rest"), &theme());
        let rows = &capture(&captures, "rest").rows;
        assert_eq!(rows.len(), ROOT_ROWS.len());
        // The pinned home above them: the label (30), the list's gap, the
        // strip (2 + 80 + 6) and the gap again.
        let home = 30. + 2. + 88. + 2.;
        for (index, row) in rows.iter().enumerate() {
            // Row i below the header, the list's top padding, the home and
            // the "Commands" label (30 and the list's gap), then one row
            // height and one list gap per row before it.
            let y = 64. + 4. + home + (30. + 2.) + index as f32 * (44. + 2.);
            assert_eq!(
                row.rect,
                Rect {
                    x: 10.,
                    y,
                    width: 760. - 20.,
                    height: 44.
                }
            );
        }
        // The first four show whole, as the reference's do; the list
        // scrolls to the rest.
        assert!(rows[..4].iter().all(|row| row.visible));
        assert!(!rows.last().unwrap().visible);
        let sections = &capture(&captures, "rest").sections;
        let labels: Vec<_> = sections.iter().map(|label| label.label.as_str()).collect();
        assert_eq!(labels, ["Pinned", "Commands"]);
        assert_eq!(sections[0].rect.y, 64. + 4.);
        assert_eq!(sections[1].rect.y, 64. + 4. + home);
    }

    #[test]
    fn the_home_lies_above_the_rows_as_the_reference_lays_it_out() {
        let captures = declared_captures(scenario("pinned-strip"), &theme());
        let rest = capture(&captures, "rest");
        let home = rest.pinned.as_ref().expect("a blank query shows the home");
        assert_eq!(home.label.y, 64. + 4.);
        assert_eq!(home.strip.y, 64. + 4. + 30. + 2.);
        assert_eq!(home.strip.height, 2. + 80. + 6.);
        // Five equal columns across the list's 740px, 8px apart.
        let column = (740. - 4. * 8.) / 5.;
        assert_eq!(home.slots.len(), 5);
        for (index, slot) in home.slots.iter().enumerate() {
            assert_eq!(
                slot.rect,
                Rect {
                    x: 10. + index as f32 * (column + 8.),
                    y: home.strip.y + 2.,
                    width: column,
                    height: 80.
                }
            );
        }
        let last = home.slots[4].rect;
        assert!((last.x + last.width - 750.).abs() < 0.01, "{last:?}");
        // The 30px tile and the 12.5px title (a 16.25px line) centered
        // down the slot's 64px inside its paddings, 7px apart: Pane's
        // compact strip.
        let first = &home.slots[0];
        let tile = first.tile;
        let top = first.rect.y + 8. + (64. - (30. + 7. + 12.5 * 1.3)) / 2.;
        assert_eq!(
            (tile.x, tile.width),
            (first.rect.x + (column - 30.) / 2., 30.)
        );
        assert!((tile.y - top).abs() < 0.01, "{tile:?}");
        assert_eq!(first.title, "Terminal");
        assert_eq!(first.tone, "app");
        let titles: Vec<_> = home.slots.iter().map(|slot| slot.title).collect();
        assert_eq!(
            titles,
            [
                "Terminal",
                "Visual Studio Code",
                "Firefox",
                "Obsidian",
                "Spotify"
            ]
        );
        // Five pins fill the strip's row: no pin hint follows them.
        assert!(home.hint.is_none());
    }

    #[test]
    fn a_query_hides_the_home_and_clearing_it_restores_it() {
        let captures = declared_captures(scenario("pinned-strip"), &theme());
        let typed = capture(&captures, "query-hides");
        assert!(typed.pinned.is_none());
        assert_eq!(typed.rows[0].rect.y, 64. + 4. + 30. + 2.);
        assert!(typed.sections.iter().all(|label| label.label != "Pinned"));
        let cleared = capture(&captures, "cleared-restores");
        assert!(cleared.pinned.is_some());
        assert_eq!(cleared.rows.len(), ROOT_ROWS.len());
    }

    #[test]
    fn pointing_at_a_slot_washes_it_and_selects_no_row() {
        let replay = replay(scenario("pinned-strip"), &theme());
        let hover = capture(&replay.captures, "slot-hover");
        let home = hover.pinned.as_ref().expect("the home shows");
        let hovered: Vec<_> = home.slots.iter().map(|slot| slot.hovered).collect();
        assert_eq!(hovered, [false, true, false, false, false]);
        assert_eq!(
            hover.rows.iter().position(|row| row.selected),
            Some(0),
            "the rows' selection stays"
        );
        let point = replay
            .steps
            .iter()
            .find(|step| matches!(step.step, Step::Point { .. }))
            .and_then(|step| step.point);
        assert_eq!(point, Some(home.slots[1].rect.center()));
    }

    #[test]
    fn a_partial_home_declares_its_pins_its_unavailable_one_and_the_pin_hint() {
        let captures = declared_captures(scenario("pinned-partial"), &theme());
        let home = capture(&captures, "partial")
            .pinned
            .as_ref()
            .expect("the home shows");
        let titles: Vec<_> = home.slots.iter().map(|slot| slot.title).collect();
        assert_eq!(titles, ["Terminal", "Firefox", "Obsidian"]);
        // The pin hint takes the fourth column of the strip's one row, a
        // slot's size: three pins leave room in that row.
        let hint = home.hint.expect("the pin hint follows the pins");
        let third = home.slots[2].rect;
        assert_eq!(hint.y, third.y);
        assert_eq!((hint.width, hint.height), (third.width, third.height));
        assert!(
            (hint.x - (third.x + third.width + 8.)).abs() < 0.01,
            "{hint:?}"
        );
        assert_eq!(home.strip.height, 2. + 80. + 6.);
        let unavailable = &home.slots[2];
        assert_eq!(unavailable.unavailable, Some("Notes is disabled"));
        // The reason's line below the title moves the content up, and the
        // tile, the title and the reason still fit inside the slot's
        // paddings, so nothing shrinks or clips.
        let tile = unavailable.tile;
        let available = home.slots[0].tile;
        assert!(tile.y < available.y);
        let theme = theme();
        let pinned = &theme.geometry.pinned;
        let reason_line =
            f32::from(theme.typography.slot_reason_size) * theme.typography.line_height;
        let title = unavailable.title_rect;
        let slot = unavailable.rect;
        assert!(tile.y >= slot.y + f32::from(pinned.slot_padding_top) - 0.01);
        assert!(
            title.y + title.height + reason_line
                <= slot.y + slot.height - f32::from(pinned.slot_padding_bottom) + 0.01
        );
    }

    #[test]
    fn the_selected_scenario_moves_one_row_per_key_and_records_each_step() {
        let captures = declared_captures(scenario("root-selected"), &theme());
        let selected: Vec<_> = captures
            .iter()
            .map(|capture| capture.rows.iter().position(|row| row.selected))
            .collect();
        assert_eq!(selected, [Some(0), Some(1), Some(2)]);
        assert_eq!(captures[2].after.len(), 2);
    }

    #[test]
    fn moving_the_pointer_onto_a_row_selects_it() {
        let captures = declared_captures(scenario("root-hover"), &theme());
        let hover = capture(&captures, "hover");
        let (x, y) = hover.pointer.expect("the pointer is in the window");
        assert!(hover.rows[1].rect.contains((x, y)));
        assert!(hover.rows[1].hovered && hover.rows[1].selected);
        assert!(!hover.rows[0].selected && !hover.rows[0].hovered);
        assert_eq!(hover.action, Some("Run Command"));
        assert!(capture(&captures, "rest").pointer.is_none());

        let captures = declared_captures(scenario("root-selected-hover"), &theme());
        let both = capture(&captures, "selected-hover");
        assert!(both.rows[2].hovered && both.rows[2].selected);
    }

    #[test]
    fn a_resting_pointer_keeps_its_wash_while_the_keys_move_the_selection() {
        let captures = declared_captures(scenario("root-pointer-keys"), &theme());
        let selected = |name: &str| {
            capture(&captures, name)
                .rows
                .iter()
                .position(|row| row.selected)
        };
        assert_eq!(selected("pointed"), Some(1));
        // Down with the pointer resting on row 1: row 2 is selected, row
        // 1 keeps the hover wash.
        let down = capture(&captures, "down-under-pointer");
        assert_eq!(selected("down-under-pointer"), Some(2));
        assert!(down.rows[1].hovered && !down.rows[1].selected);
        // Moving again over row 1, a few pixels on, selects it again.
        assert_eq!(selected("moved-again"), Some(1));
    }

    #[test]
    fn the_footer_action_follows_the_selected_rows_kind() {
        // The reference names the primary action by the selected row's
        // kind: Figma is an application, Clipboard History a command.
        let captures = declared_captures(scenario("root-selected"), &theme());
        let actions: Vec<_> = captures.iter().map(|capture| capture.action).collect();
        assert_eq!(
            actions,
            [
                Some("Open Application"),
                Some("Run Command"),
                Some("Run Command")
            ]
        );
    }

    #[test]
    fn the_actions_scenario_opens_filters_empties_and_closes_the_panel_only() {
        let captures = declared_captures(scenario("root-actions"), &theme());
        let names: Vec<_> = captures.iter().map(|capture| capture.name).collect();
        assert_eq!(names, ["selected", "open", "filtered", "empty", "closed"]);
        let labels = |capture: &DeclaredCapture| -> Vec<String> {
            capture
                .actions
                .as_ref()
                .map(|panel| panel.rows.iter().map(|row| row.label.clone()).collect())
                .unwrap_or_default()
        };
        // Clipboard History can be pinned, and has an alias and a hotkey:
        // its configuration entries offer to change them, under the
        // "Pane" label.
        assert_eq!(
            labels(&captures[1]),
            ["Run Command", "Pin", "Change Hotkey…", "Change Alias…"]
        );
        let open = captures[1].actions.as_ref().expect("the panel is open");
        assert_eq!((open.groups.len(), open.rules.len()), (1, 1));
        assert!(open.rows[0].selected);
        // The filter narrows them, and drops the group's rule and label.
        assert_eq!(labels(&captures[2]), ["Change Alias…"]);
        let filtered = captures[2].actions.as_ref().expect("still open");
        assert_eq!((filtered.groups.len(), filtered.rules.len()), (0, 0));
        // Then to nothing, which the panel says.
        let empty = captures[3].actions.as_ref().expect("still open");
        assert!(empty.rows.is_empty());
        assert_eq!(
            empty.empty.as_ref().map(|note| note.text.as_str()),
            Some(actions_panel::NO_MATCH)
        );
        // Escape closes the panel only: the query and the target stay.
        assert!(captures[4].actions.is_none());
        for capture in &captures {
            assert_eq!(capture.query, "");
            assert_eq!(capture.action, Some("Run Command"));
        }
    }

    #[test]
    fn the_declared_panel_sits_above_the_footer_at_the_right() {
        let theme = theme();
        let frame = frame(&theme, ROOT_CLIENT);
        let captures = declared_captures(scenario("root-actions"), &theme);
        let panel = captures[1].actions.as_ref().expect("the panel is open");
        assert_eq!(panel.rect.x + panel.rect.width, ROOT_CLIENT.0 - 10.);
        assert_eq!(panel.rect.width, 320.);
        assert_eq!(panel.rect.y + panel.rect.height, frame.footer.y - 8.);
        assert_eq!(
            panel.search.y + panel.search.height,
            panel.rect.y + panel.rect.height,
            "the search row closes the panel"
        );
        let header = panel.header.as_ref().expect("the target is named");
        assert_eq!(header.rect.y, panel.rect.y);
        // Header 30, list padding 6: the first entry; then 36 and the 1px
        // gap, the rule's 9 and the gap, the label's 26 and the gap.
        assert_eq!(panel.rows[0].rect.y, panel.rect.y + 36.);
        assert_eq!(panel.rules[0].y, panel.rows[0].rect.y + 36. + 1. + 4.);
        assert_eq!(
            panel.groups[0].rect.y,
            panel.rows[0].rect.y + 36. + 1. + 9. + 1.
        );
        assert_eq!(panel.rows[1].rect.y, panel.groups[0].rect.y + 26. + 1.);
        assert_eq!(panel.dimmer, frame.list);
    }

    #[test]
    fn pointer_steps_resolve_to_the_center_of_their_row() {
        let replay = replay(scenario("root-hover"), &theme());
        let pointer = replay
            .steps
            .iter()
            .find(|step| matches!(step.step, Step::Pointer { .. }))
            .expect("the hover scenario moves the pointer");
        let row = &replay.captures[1].rows[1].rect;
        assert_eq!(pointer.point, Some(row.center()));
        assert!(
            replay
                .steps
                .iter()
                .filter(|step| step.point.is_some())
                .count()
                == 1
        );
    }

    #[test]
    fn typing_filters_by_title_and_escape_returns_to_rest() {
        let captures = declared_captures(scenario("root-focus"), &theme());
        let typed = capture(&captures, "focus-typed");
        assert_eq!(typed.query, "clip");
        let titles: Vec<_> = typed.rows.iter().map(|row| row.title).collect();
        assert_eq!(titles, ["Clipboard History"]);
        assert!(typed.rows[0].selected);
        let back = capture(&captures, "back-to-rest");
        assert_eq!(back.query, "");
        assert_eq!(back.rows.len(), ROOT_ROWS.len());
        assert!(back.rows[0].selected);
    }

    #[test]
    fn rows_after_an_unavailable_reason_declare_only_a_height_floor() {
        let captures = declared_captures(scenario("root-unavailable"), &theme());
        let rows = &capture(&captures, "rest").rows;
        assert!(!rows[0].height_is_floor);
        assert!(rows[1].height_is_floor && rows[1].unavailable.is_some());
    }

    #[test]
    fn the_perturbations_change_exactly_the_value_they_name() {
        let base = theme();
        let mut padded = theme();
        Perturbation::RowPaddingPlus4.apply(&mut padded);
        assert_eq!(
            padded.geometry.list_padding_x,
            base.geometry.list_padding_x + px(4.)
        );
        assert_eq!(hex(padded.row_selected), hex(base.row_selected));

        let mut filled = theme();
        Perturbation::SelectedFill.apply(&mut filled);
        assert_eq!(hex(filled.row_selected), "#FFFFFF40");
        assert_ne!(hex(base.row_selected), "#FFFFFF40");
        assert_eq!(filled.geometry.list_padding_x, base.geometry.list_padding_x);

        let mut hovered = theme();
        Perturbation::HoverFill.apply(&mut hovered);
        assert_eq!(hex(hovered.row_hover), "#FFFFFF33");
        assert_eq!(hex(hovered.row_selected), hex(base.row_selected));

        let mut section = theme();
        Perturbation::NavSelectedFill.apply(&mut section);
        assert_eq!(hex(section.nav_selected), "#FFFFFF40");
        assert_ne!(hex(base.nav_selected), "#FFFFFF40");
        assert_eq!(hex(section.row_selected), hex(base.row_selected));
        for &(name, perturbation) in PERTURBATIONS {
            assert_eq!(Perturbation::parse(name), Ok(Some(perturbation)));
            assert_eq!(perturbation.name(), name);
        }
    }

    #[test]
    fn arguments_parse_into_a_run_or_a_registry() {
        let args = |text: &str| text.split(' ').map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(
            parse_args(args(
                "--scenario root-rest --manifest m.json --perturb selected-fill --material opaque"
            )),
            Ok(Command::Run(FixtureOptions {
                scenario: "root-rest".into(),
                data_dir: None,
                theme: None,
                material: Some("opaque".into()),
                perturbation: Some(Perturbation::SelectedFill),
                manifest: "m.json".into(),
            }))
        );
        assert_eq!(
            parse_args(args("--registry r.json")),
            Ok(Command::Registry("r.json".into()))
        );
        assert!(parse_args(args("--scenario root-rest")).is_err());
        assert!(parse_args(args("--perturb sideways")).is_err());
        assert!(parse_args(args("--manifest")).is_err());
    }

    fn clipboard_of(capture: &DeclaredCapture) -> &DeclaredClipboard {
        capture.clipboard.as_ref().expect("a clipboard capture")
    }

    fn clip_titles(clipboard: &DeclaredClipboard) -> Vec<&'static str> {
        clipboard.rows.iter().map(|row| row.title).collect()
    }

    fn selected_clip(clipboard: &DeclaredClipboard) -> Option<&'static str> {
        clipboard
            .rows
            .iter()
            .find(|row| row.selected)
            .map(|row| row.id)
    }

    #[test]
    fn the_split_view_divides_the_clipboard_board_as_the_reference_does() {
        let frame = clip_frame(&theme(), CLIPBOARD_CLIENT);
        // 64 header + 46 tabs + 438 body + 52 footer = 600.
        assert_eq!(frame.tabs.y, 64.);
        assert_eq!(
            frame.list,
            Rect {
                x: 0.,
                y: 110.,
                width: 360.,
                height: 438.
            }
        );
        assert_eq!(frame.footer.y, 548.);
        // The preview card: the pane's 12px padding around 556x414.
        assert_eq!(
            frame.card,
            Rect {
                x: 372.,
                y: 122.,
                width: 556.,
                height: 414.
            }
        );
        // A narrower window keeps the list at most half its width.
        let narrow = clip_frame(&theme(), (600., 518.));
        assert_eq!(narrow.list.width, 300.);
    }

    #[test]
    fn clipboard_rows_and_labels_lie_where_the_reference_board_puts_them() {
        let captures = declared_captures(scenario("clipboard-rest"), &theme());
        let clipboard = clipboard_of(&captures[0]);
        // The reference's DOM, relative to its glass: the Pinned label at
        // y 112, ssh at 144, Today at 190, the code clip at 222 — 343 wide
        // from x 8 — and the Yesterday label at 452.
        let tops: Vec<f32> = clipboard
            .sections
            .iter()
            .map(|label| label.rect.y)
            .collect();
        assert_eq!(tops, [112., 190., 452.]);
        let rows: Vec<(f32, f32, f32)> = clipboard
            .rows
            .iter()
            .map(|row| (row.rect.x, row.rect.y, row.rect.width))
            .collect();
        assert_eq!(rows[0], (8., 144., 343.));
        assert_eq!(rows[1], (8., 222., 343.));
        assert_eq!(rows[6].1, 484.);
        assert_eq!(selected_clip(clipboard), Some("code"));
        // The last clip runs past the list's bottom, as the reference's.
        assert!(!clipboard.rows[7].visible && clipboard.rows[6].visible);
        assert_eq!(
            clipboard.preview.as_ref().map(|preview| preview.kind),
            Some("code")
        );
        assert_eq!(
            clipboard.copied.text,
            "Copied today, 14:02 from Visual Studio Code"
        );
    }

    #[test]
    fn a_click_selects_a_clip_and_the_preview_follows_it() {
        let replay = replay(scenario("clipboard-previews"), &theme());
        let kinds: Vec<_> = replay
            .captures
            .iter()
            .map(|capture| clipboard_of(capture).preview.as_ref().map(|p| p.kind))
            .collect();
        assert_eq!(
            kinds,
            [Some("text"), Some("color"), Some("link"), Some("image")]
        );
        let text = clipboard_of(&replay.captures[0]);
        assert_eq!(selected_clip(text), Some("standup"));
        assert_eq!(text.copied.text, "Copied today, 11:04 from Slack");
        // Each click lands on its row's center.
        let click = &replay.steps[0];
        let standup = text.rows.iter().find(|row| row.id == "standup").unwrap();
        assert_eq!(click.point, Some(standup.rect.center()));
    }

    #[test]
    fn the_keys_reach_the_last_clip_and_scroll_it_into_view() {
        let captures = declared_captures(scenario("clipboard-keys"), &theme());
        assert_eq!(selected_clip(clipboard_of(&captures[0])), Some("lime"));
        let last = clipboard_of(&captures[1]);
        let ice = last.rows.last().unwrap();
        assert_eq!(selected_clip(last), Some("ice"));
        assert!(ice.visible);
        // Scrolled by the least that shows it: its bottom on the list's.
        assert_eq!(ice.rect.y + ice.rect.height, 548.);
    }

    #[test]
    fn a_query_no_clip_matches_lists_nothing_and_previews_nothing() {
        let captures = declared_captures(scenario("clipboard-filter"), &theme());
        let none = clipboard_of(&captures[0]);
        assert!(none.rows.is_empty() && none.preview.is_none());
        assert_eq!(
            none.empty.as_ref().map(|note| note.text.as_str()),
            Some("No clips match. Try another filter.")
        );
        assert_eq!(none.copied.text, "Nothing selected");
        assert_eq!(clipboard_of(&captures[1]).rows.len(), 8);
        let text = clipboard_of(&captures[2]);
        assert_eq!(text.tab, "Text");
        assert_eq!(
            clip_titles(text),
            [
                "ssh deploy@10.0.4.12",
                "const pane = createPane({",
                "Standup moved to 10:30 tomorrow",
                "hello@example.com"
            ]
        );
        assert_eq!(selected_clip(text), Some("code"));
    }

    #[test]
    fn production_scenarios_show_production_content_only() {
        let captures = declared_captures(scenario("clipboard-production"), &theme());
        let rest = clipboard_of(&captures[0]);
        assert_eq!(rest.variant, "production");
        let labels: Vec<_> = rest
            .sections
            .iter()
            .map(|label| label.label.as_str())
            .collect();
        assert_eq!(labels, ["Today", "Yesterday", "Older"]);
        assert!(rest.caption.starts_with("Text is kept for 7 days"));
        assert_eq!(
            selected_clip(clipboard_of(&captures[1])),
            Some("5"),
            "the long record"
        );
        let off = declared_captures(scenario("clipboard-off"), &theme());
        let off = clipboard_of(&off[0]);
        assert!(off.rows.is_empty() && off.preview.is_none());
        assert_eq!(off.capture.text, "Turn on");
        assert!(
            off.empty
                .as_ref()
                .unwrap()
                .text
                .starts_with("Clipboard history is off")
        );
        assert_eq!(ClipVariant::Production.tabs(), ["All", "Text"]);
    }

    #[test]
    fn the_registry_serializes_steps_for_the_capture_helpers() {
        let json = serde_json::to_value(Registry {
            kind: "pane-visual-fixture-registry",
            scenarios: scenarios(),
            pending: pending_scenarios(),
        })
        .unwrap();
        let hover = &json["scenarios"][1];
        assert_eq!(hover["name"], "root-hover");
        assert_eq!(
            hover["steps"][1],
            serde_json::json!({ "action": "pointer", "row": 1, "nudge": 0 })
        );
        assert_eq!(hover["client"], serde_json::json!([760.0, 518.0]));
    }

    /// The empty board (#96): its query typed, the notice (84) heads the
    /// list, the fallbacks follow under their label, none selected until
    /// Down selects the first, whose action the footer then names.
    #[test]
    fn the_empty_board_heads_its_unselected_fallbacks_with_the_notice() {
        let captures = declared_captures(scenario("empty-state"), &theme());
        let notice = capture(&captures, "notice");
        assert_eq!(notice.query, "kubectx");
        assert_eq!(notice.rows.len(), EMPTY_ROWS.len());
        assert!(notice.rows.iter().all(|row| !row.selected));
        assert_eq!(notice.action, None);
        assert_eq!(notice.sections[0].label, "Fallbacks");
        assert_eq!(notice.sections[0].rect.y, 64. + 4. + 84. + 2.);
        assert_eq!(notice.rows[0].rect.y, 64. + 4. + 84. + 2. + 30. + 2.);
        let selected = capture(&captures, "fallback-selected");
        assert!(selected.rows[0].selected);
        assert_eq!(selected.action, Some("Search Web"));
    }

    /// The calculator board (#96): its labels over its rows, the first
    /// drawn as the selected card at the reference's 740x160 at (10, 102),
    /// the history rows under the second label.
    #[test]
    fn the_calculator_board_lays_its_card_out_in_its_first_rows_place() {
        let captures = declared_captures(scenario("calculator-card"), &theme());
        let card = capture(&captures, "card");
        assert_eq!(card.query, "72 in to cm");
        let labels: Vec<_> = card
            .sections
            .iter()
            .map(|section| section.label.as_str())
            .collect();
        assert_eq!(labels, ["Calculator", "Recent calculations"]);
        assert!(card.rows[0].answer && card.rows[0].selected);
        assert_eq!(
            card.rows[0].rect,
            Rect {
                x: 10.,
                y: 102.,
                width: 740.,
                height: 160.
            }
        );
        // The card's 4 below it and the list's gap, then the label.
        assert_eq!(card.sections[1].rect.y, 102. + 160. + 4. + 2.);
        assert_eq!(card.rows[1].rect.y, 268. + 30. + 2.);
        assert_eq!(card.rows.iter().filter(|row| row.answer).count(), 1);
        assert_eq!(card.rows[3].rect.height, 44.);
        assert_eq!(card.action, Some("Copy Answer"));
    }

    /// A computed answer as production presents it (#96) sits under its
    /// command's title, by root search's own rule, on a card with no
    /// captions or chips: its paddings around one 44px line.
    #[test]
    fn a_production_answer_sits_under_its_commands_title() {
        let captures = declared_captures(scenario("answer-plain"), &theme());
        let answer = capture(&captures, "answer");
        assert_eq!(answer.query, "6*7");
        assert_eq!(answer.sections.len(), 1);
        assert_eq!(answer.sections[0].label, "Calculator");
        assert_eq!(answer.rows[0].rect.height, 20. + 44. + 16.);
    }
}
