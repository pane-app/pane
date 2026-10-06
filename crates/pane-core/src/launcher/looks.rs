//! How rows look beyond their titles (#139): the icon of an installed
//! command's row in root search (its own, its package's, or its package's
//! first-letter tile), and an open command's items' icons, tooltips and
//! accessories, as the window draws them.
//!
//! A command's tree names its images relative to its package; they are
//! resolved against the package's managed copy once, when the list is
//! drawn, and kept by item id ([`Looks`]) for [`super::Launcher::presentation`]
//! to project row for row. A date accessory is kept as a time and shown
//! relative to the launcher's clock each time the rows are presented, so it
//! stays current while the list is open; its tooltip is the absolute time.
//! A row shows at most [`MAX_ACCESSORIES`]; while a package is developed,
//! an item with more is reported in the status line.

use std::collections::HashMap;
use std::path::Path;

use super::clipboard_view::{clock_time, local_day, local_offset_ms, month_and_day};
use super::{Launcher, State, Status, choices, owner};
use crate::icons::{Icon, Tint};
use crate::runtime::{Accessory, AccessoryContent, Item, ItemLook, MAX_ACCESSORIES};

/// The open command's items' looks, by item id, their images resolved.
#[derive(Clone, Debug, Default)]
pub(super) struct Looks {
    by_item: HashMap<String, ItemLook>,
    /// The items with more accessories than a row draws, as last reported
    /// while their package is developed.
    reported: Vec<(String, usize)>,
}

impl Looks {
    /// The look of the item with id `id`, if the open command's list has it.
    pub(super) fn of(&self, id: &str) -> Option<&ItemLook> {
        self.by_item.get(id)
    }
}

/// How an accessory is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessoryKind {
    /// Text, such as a count.
    Text,
    /// A time, shown relative to now ("2h").
    Date,
    /// A coloured tag.
    Tag,
}

/// One accessory as a row shows it now (#139).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownAccessory {
    pub kind: AccessoryKind,
    /// The text drawn: a date's relative to now ("2h"); empty for an icon
    /// alone.
    pub text: String,
    pub icon: Option<Icon>,
    /// The colour of its text or tag, before Pane corrects its contrast.
    pub color: Option<Tint>,
    /// Shown on hover: the tree's, or a date's absolute time.
    pub tooltip: Option<String>,
}

impl ShownAccessory {
    /// What assistive technology reads of it with its row: its tooltip
    /// for a date (the absolute time), else its text, with its tooltip
    /// after it; its icon's tooltip when it is an icon alone.
    pub fn spoken(&self) -> String {
        let text = match self.kind {
            AccessoryKind::Date => self.tooltip.clone().unwrap_or_else(|| self.text.clone()),
            _ => match &self.tooltip {
                Some(tooltip) if !self.text.is_empty() => format!("{}, {tooltip}", self.text),
                Some(tooltip) => tooltip.clone(),
                None => self.text.clone(),
            },
        };
        if text.is_empty() {
            return self
                .icon
                .as_ref()
                .and_then(|icon| icon.tooltip.clone())
                .unwrap_or_default();
        }
        text
    }
}

/// Keeps the looks of `items`, the open command's list in `component`
/// drawn (again), their images resolved in its package's managed copy
/// (or, for a command built into Pane, beside its component). Answers the
/// items with more accessories than a row draws, by title, with how many
/// they have, for [`Launcher::report_extra_accessories`].
pub(super) fn remember(
    state: &mut State,
    component: &Path,
    items: &[Item],
) -> Vec<(String, usize)> {
    let folder = owner(&state.packages, component)
        .map(|package| package.location.clone())
        .or_else(|| component.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    state.looks.by_item = items
        .iter()
        .map(|item| (item.id.clone(), resolved(item.look.clone(), &folder)))
        .collect();
    let mut extra: Vec<(String, usize)> = items
        .iter()
        .filter(|item| item.look.accessories.len() > MAX_ACCESSORIES)
        .map(|item| (item.title.clone(), item.look.accessories.len()))
        .collect();
    extra.sort();
    extra
}

/// `look` with its icons resolved in `folder` (see [`Icon::resolved`]).
fn resolved(look: ItemLook, folder: &Path) -> ItemLook {
    let icon = |icon: Option<Icon>| icon.and_then(|icon| icon.resolved(folder));
    ItemLook {
        icon: icon(look.icon),
        title_tooltip: look.title_tooltip,
        subtitle_tooltip: look.subtitle_tooltip,
        accessories: look
            .accessories
            .into_iter()
            .filter_map(|accessory| {
                let icon = icon(accessory.icon);
                // An icon alone that cannot be drawn shows nothing.
                if icon.is_none()
                    && matches!(&accessory.content, AccessoryContent::Text(text) if text.is_empty())
                {
                    return None;
                }
                Some(Accessory { icon, ..accessory })
            })
            .collect(),
        action_icons: look.action_icons.into_iter().map(icon).collect(),
    }
}

/// The icon of the installed command with id `id` (`<package key>#<id in
/// its manifest>`), or of the installed package with key `id`: the
/// command's own, else its package's, else its package's first-letter
/// tile. `None` for anything else, such as one of Pane's own rows.
pub(super) fn icon_of(state: &State, id: &str) -> Option<Icon> {
    if let Some(package) = state.packages.iter().find(|p| p.identity.key() == id) {
        return Some(package.icon().clone());
    }
    let (key, command) = choices::split(id);
    let package = state.packages.iter().find(|p| p.identity.key() == key)?;
    let manifest = package.manifest.as_ref().ok()?;
    manifest
        .commands
        .iter()
        .any(|declared| declared.id == command)
        .then(|| package.command_icon(command).clone())
}

/// The accessories `look` shows now, by the clock's `now` (milliseconds
/// since the Unix epoch): the first [`MAX_ACCESSORIES`], a date's text
/// relative to now and its tooltip the absolute time.
pub(super) fn shown_accessories(look: &ItemLook, now: u64) -> Vec<ShownAccessory> {
    look.accessories
        .iter()
        .take(MAX_ACCESSORIES)
        .map(|accessory| {
            let (kind, text, tooltip) = match &accessory.content {
                AccessoryContent::Text(text) => {
                    (AccessoryKind::Text, text.clone(), accessory.tooltip.clone())
                }
                AccessoryContent::Tag(tag) => {
                    (AccessoryKind::Tag, tag.clone(), accessory.tooltip.clone())
                }
                AccessoryContent::Date(at) => {
                    let now = i64::try_from(now).unwrap_or(i64::MAX);
                    let at_ms = u64::try_from(*at).unwrap_or(0);
                    let absolute = absolute_date(*at, local_offset_ms(at_ms));
                    (
                        AccessoryKind::Date,
                        relative_date(*at, now),
                        Some(accessory.tooltip.clone().unwrap_or(absolute)),
                    )
                }
            };
            ShownAccessory {
                kind,
                text,
                icon: accessory.icon.clone(),
                color: accessory.color,
                tooltip,
            }
        })
        .collect()
}

/// `at` relative to `now` (both milliseconds since the Unix epoch), as an
/// accessory shows it: "now" within a minute, then "5m", "2h", "3d", "2w",
/// "4mo" and "1y", each rounded down; a time to come is "in 5m".
pub fn relative_date(at: i64, now: i64) -> String {
    const MINUTE: i64 = 60_000;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    const WEEK: i64 = 7 * DAY;
    const MONTH: i64 = 30 * DAY;
    const YEAR: i64 = 365 * DAY;
    let apart = now.saturating_sub(at);
    let span = apart.saturating_abs();
    let amount = match span {
        ..MINUTE => return "now".into(),
        ..HOUR => format!("{}m", span / MINUTE),
        ..DAY => format!("{}h", span / HOUR),
        ..WEEK => format!("{}d", span / DAY),
        ..MONTH => format!("{}w", span / WEEK),
        ..YEAR => format!("{}mo", (span / MONTH).max(1)),
        _ => format!("{}y", span / YEAR),
    };
    if apart < 0 {
        format!("in {amount}")
    } else {
        amount
    }
}

/// `at` (milliseconds since the Unix epoch) as a date accessory's tooltip
/// says it, at `offset_ms` from UTC: "Oct 7, 2026, 14:02".
pub fn absolute_date(at: i64, offset_ms: i64) -> String {
    let at = u64::try_from(at).unwrap_or(0);
    let day = local_day(at, offset_ms);
    let (month, date, year) = month_and_day(day);
    format!("{month} {date}, {year}, {}", clock_time(at, offset_ms))
}

impl Launcher {
    /// The icon of the installed command with id `id` (`<package key>#<id
    /// in its manifest>`, as root search, quick slots and the Shortcuts
    /// page name it), or of the installed package with key `id` (#139):
    /// the command's own, else its package's, else its package's
    /// first-letter tile. `None` for anything else, such as Pane's own
    /// rows, which keep their tiles.
    pub fn icon_of(&self, id: &str) -> Option<Icon> {
        icon_of(&self.lock(), id)
    }

    /// While the open command's package is developed, says in the status
    /// line which of its items have more accessories than a row draws
    /// (`extra`, as [`remember`] answered), when that changed since it last
    /// said, or, `opened`, whenever the command has just opened.
    pub(super) fn report_extra_accessories(
        &self,
        state: &mut State,
        extra: Vec<(String, usize)>,
        opened: bool,
    ) {
        if opened {
            state.looks.reported = Vec::new();
        }
        let developed = state
            .open
            .as_ref()
            .and_then(|component| owner(&state.packages, component))
            .is_some_and(|package| self.is_developed(&package.identity));
        if developed && !extra.is_empty() && extra != state.looks.reported {
            let items: Vec<String> = extra
                .iter()
                .map(|(title, count)| format!("“{title}” has {count}"))
                .collect();
            state.view.status = Status::Error(format!(
                "Accessories Pane did not draw: a row shows at most {MAX_ACCESSORIES}, and {}",
                items.join("; ")
            ));
        }
        state.looks.reported = extra;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60_000;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;

    #[test]
    fn a_date_shows_relative_to_now_and_in_full_in_its_tooltip() {
        let now = 1_767_225_600_000; // 2026-01-01T00:00:00Z
        for (ago, shown) in [
            (0, "now"),
            (59_000, "now"),
            (5 * MINUTE, "5m"),
            (2 * HOUR + 59 * MINUTE, "2h"),
            (3 * DAY, "3d"),
            (15 * DAY, "2w"),
            (100 * DAY, "3mo"),
            (800 * DAY, "2y"),
            (-5 * MINUTE, "in 5m"),
        ] {
            assert_eq!(relative_date(now - ago, now), shown, "{ago}");
        }
        assert_eq!(
            absolute_date(now + 14 * HOUR + 2 * MINUTE, 0),
            "Jan 1, 2026, 14:02"
        );
        assert_eq!(absolute_date(now, -HOUR), "Dec 31, 2025, 23:00");
    }

    #[test]
    fn a_row_shows_three_accessories_with_dates_current_and_reads_them() {
        let now = 1_767_225_600_000u64;
        let look = ItemLook {
            accessories: vec![
                Accessory {
                    content: AccessoryContent::Text("3".into()),
                    icon: None,
                    color: None,
                    tooltip: Some("Unread".into()),
                },
                Accessory {
                    content: AccessoryContent::Date(now as i64 - 2 * HOUR),
                    icon: None,
                    color: None,
                    tooltip: None,
                },
                Accessory {
                    content: AccessoryContent::Tag("Open".into()),
                    icon: None,
                    color: None,
                    tooltip: None,
                },
                Accessory {
                    content: AccessoryContent::Text("extra".into()),
                    icon: None,
                    color: None,
                    tooltip: None,
                },
            ],
            ..ItemLook::default()
        };
        let shown = shown_accessories(&look, now);
        assert_eq!(shown.len(), MAX_ACCESSORIES);
        assert_eq!(
            shown.iter().map(|a| a.text.as_str()).collect::<Vec<_>>(),
            ["3", "2h", "Open"]
        );
        assert_eq!(shown[0].spoken(), "3, Unread");
        assert!(shown[1].tooltip.is_some());
        assert_eq!(shown[1].spoken(), shown[1].tooltip.clone().unwrap());
        // An hour on, the date has moved.
        assert_eq!(shown_accessories(&look, now + HOUR as u64)[1].text, "3h");
    }
}
