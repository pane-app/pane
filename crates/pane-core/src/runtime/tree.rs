//! What a command's screen is, as its `render` answers it: a versioned JSON
//! tree (ADR 0036's envelope; `docs/list-tree.md` describes every field),
//! and what its `handle-event` answers. Reading them here keeps the JSON at
//! the edge of the runtime: the launcher sees only the typed [`View`] and
//! [`Answer`].
//!
//! Reading is lenient where a newer tree may say more and strict where the
//! tree says something wrong. A field Pane does not know is ignored, and a
//! tree naming a newer version than [`TREE_VERSION`] is read for what Pane
//! understands of it; a tree that is not JSON, lacks a field Pane needs or
//! gives one of the wrong type is unreadable, which the runtime answers as
//! [`super::CallError::Unreadable`]: the command's failure, never a crash.

use crate::icons::{self, Icon, Tint};
use serde::Deserialize;
use serde_json::Value;

use super::{Choice, CustomViewInfo, CustomViewRole, Field, FieldKind, Form};
use crate::platform::Platform;

/// The version of the tree this Pane knows: a tree names the version of
/// the component set it uses, and one naming a newer version is still
/// drawn as far as Pane understands it.
pub const TREE_VERSION: u64 = 1;

/// A command's list view, as its tree describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub title: String,
    pub items: Vec<Item>,
}

/// One entry in a command's list view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// Identifies the item among the list's items: Pane keeps the selection
    /// on it when the list is drawn again, and passes it to `submit-form`
    /// and `open-view`.
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    /// What choosing the item does, first the one Enter runs. The list has
    /// one action per item for now: Pane runs the first and ignores the
    /// others.
    pub actions: Vec<Action>,
    /// When set, activating the item opens this form instead of running its
    /// action.
    pub form: Option<Form>,
    /// The operating systems the item's action works on; `None` for every
    /// system.
    pub platforms: Option<Vec<Platform>>,
    /// When set (and `form` is not), activating the item opens this custom
    /// view instead of running its action.
    pub custom_view: Option<CustomViewInfo>,
    /// How the item looks beyond its title and subtitle: its icon,
    /// tooltips and accessories (#139).
    pub look: ItemLook,
}

/// How an item looks beyond its title and subtitle (#139): its icon, the
/// tooltips of its title and subtitle, its accessories and its actions'
/// icons, as the tree gives them. Packaged images are named relative to
/// the package folder here; the launcher resolves them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemLook {
    pub icon: Option<Icon>,
    pub title_tooltip: Option<String>,
    pub subtitle_tooltip: Option<String>,
    /// Every accessory the tree gives, in order; a row draws the first
    /// [`MAX_ACCESSORIES`].
    pub accessories: Vec<Accessory>,
    /// The icon of each of the item's actions, in the order of
    /// [`Item::actions`]: `None` for an action without one. Kept here so
    /// that the Actions panel can draw them beside the actions.
    pub action_icons: Vec<Option<Icon>>,
}

/// How many accessories a row draws; extras are not drawn, and Pane says
/// so while the package is developed. Proposed (#139).
pub const MAX_ACCESSORIES: usize = 3;

/// One accessory on the right of a row (#139).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accessory {
    pub content: AccessoryContent,
    /// Drawn before the text; an accessory may be an icon alone.
    pub icon: Option<Icon>,
    /// The colour of its text, or of a tag; Pane corrects its contrast.
    pub color: Option<Tint>,
    /// Shown on hover; a date's is its absolute time unless the tree gives
    /// one.
    pub tooltip: Option<String>,
}

/// What an accessory shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccessoryContent {
    /// Text, such as a count; empty for an icon alone.
    Text(String),
    /// A time, in milliseconds since the Unix epoch, shown relative to now
    /// ("2h") and kept current while the list is open.
    Date(i64),
    /// A coloured tag, such as "Open".
    Tag(String),
}

impl ItemLook {
    /// The look of `item`, as its tree gives it: what Pane cannot read of
    /// an icon or an accessory is left out.
    fn of(item: &WireItem) -> ItemLook {
        ItemLook {
            icon: item.icon.as_ref().and_then(icons::read),
            title_tooltip: item.title_tooltip.clone(),
            subtitle_tooltip: item.subtitle_tooltip.clone(),
            accessories: item
                .accessories
                .iter()
                .flatten()
                .filter_map(read_accessory)
                .collect(),
            action_icons: item
                .actions
                .iter()
                .flatten()
                .map(|action| action.icon.as_ref().and_then(icons::read))
                .collect(),
        }
    }
}

/// Reads one accessory: `{"text": …}`, `{"date": <ms>}` or `{"tag": …}`,
/// with an optional `icon`, `color` and `tooltip`, or an icon alone.
/// `None` for one Pane cannot read.
fn read_accessory(value: &Value) -> Option<Accessory> {
    let Value::Object(fields) = value else {
        return None;
    };
    let text = |key: &str| match fields.get(key) {
        Some(Value::String(text)) => Some(text.clone()),
        _ => None,
    };
    let icon = fields.get("icon").and_then(icons::read);
    let content = if let Some(tag) = text("tag") {
        AccessoryContent::Tag(tag)
    } else if let Some(date) = fields.get("date").and_then(Value::as_f64) {
        AccessoryContent::Date(date as i64)
    } else if let Some(text) = text("text") {
        AccessoryContent::Text(text)
    } else if icon.is_some() {
        AccessoryContent::Text(String::new())
    } else {
        return None;
    };
    Some(Accessory {
        content,
        icon,
        color: fields
            .get("color")
            .and_then(|color| icons::read_tint(color).ok()),
        tooltip: text("tooltip"),
    })
}

impl Item {
    /// The action Enter runs: the first, if the item has any.
    pub fn action(&self) -> Option<&Action> {
        self.actions.first()
    }
}

/// An action of an item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    /// What the action is called; not shown yet.
    pub title: Option<String>,
    /// The callback id Pane passes to the command's `handle-event` when the
    /// user chooses the action.
    pub callback: String,
}

/// What a command's `handle-event` answered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    /// Text shown to the user as the result, in the status line; `None`
    /// when the command shows none. Transitional: toasts replace it.
    pub status: Option<String>,
}

/// The view `tree` describes, or why Pane cannot read it.
pub(crate) fn read_view(tree: &str) -> Result<View, String> {
    let tree: Tree = serde_json::from_str(tree).map_err(|error| format!("its view: {error}"))?;
    if tree.version == 0 {
        return Err("its view names version 0; the first version is 1".into());
    }
    let kind = match tree.view.get("type") {
        Some(Value::String(kind)) => kind.clone(),
        Some(_) => return Err("its view's `type` is not a string".into()),
        None => return Err("its view has no `type`".into()),
    };
    if kind != "list" {
        return Err(format!(
            "it shows a `{kind}` view, which this version of Pane cannot show"
        ));
    }
    let list: List =
        serde_json::from_value(tree.view).map_err(|error| format!("its list: {error}"))?;
    Ok(View {
        title: list.title,
        items: list.items.into_iter().map(Item::from).collect(),
    })
}

/// What `answer`, the text a command's `handle-event` answered, says, or
/// why Pane cannot read it.
pub(crate) fn read_answer(answer: &str) -> Result<Answer, String> {
    let answer: WireAnswer =
        serde_json::from_str(answer).map_err(|error| format!("its answer: {error}"))?;
    Ok(Answer {
        status: answer.status,
    })
}

/// The tree's root: its version and its view, whose type decides how the
/// rest is read.
#[derive(Deserialize)]
struct Tree {
    version: u64,
    view: Value,
}

#[derive(Deserialize)]
struct List {
    title: String,
    items: Vec<WireItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireItem {
    id: String,
    title: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    actions: Option<Vec<WireAction>>,
    #[serde(default)]
    form: Option<WireForm>,
    /// Names of systems; a name Pane does not know is ignored.
    #[serde(default)]
    platforms: Option<Vec<String>>,
    #[serde(default)]
    custom_view: Option<WireCustomView>,
    /// Read leniently by [`icons::read`]: an icon Pane cannot draw leaves
    /// the item without one, never the tree unreadable.
    #[serde(default)]
    icon: Option<Value>,
    #[serde(default)]
    title_tooltip: Option<String>,
    #[serde(default)]
    subtitle_tooltip: Option<String>,
    /// Each read leniently by `read_accessory`.
    #[serde(default)]
    accessories: Option<Vec<Value>>,
}

impl From<WireItem> for Item {
    fn from(item: WireItem) -> Item {
        let look = ItemLook::of(&item);
        Item {
            id: item.id,
            title: item.title,
            subtitle: item.subtitle,
            actions: item
                .actions
                .unwrap_or_default()
                .into_iter()
                .map(|action| Action {
                    title: action.title,
                    callback: action.on_action,
                })
                .collect(),
            form: item.form.map(Form::from),
            platforms: item.platforms.map(|names| {
                names
                    .iter()
                    .filter_map(|name| match name.as_str() {
                        "windows" => Some(Platform::Windows),
                        "macos" => Some(Platform::Macos),
                        "linux" => Some(Platform::Linux),
                        _ => None,
                    })
                    .collect()
            }),
            custom_view: item.custom_view.map(|view| CustomViewInfo {
                title: view.title,
                label: view.label,
                role: match view.role {
                    WireRole::ColorWell => CustomViewRole::ColorWell,
                },
            }),
            look,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireAction {
    /// Read leniently into [`ItemLook::action_icons`].
    #[serde(default)]
    icon: Option<Value>,
    #[serde(default)]
    title: Option<String>,
    on_action: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireForm {
    title: String,
    fields: Vec<WireField>,
    submit_label: String,
}

impl From<WireForm> for Form {
    fn from(form: WireForm) -> Form {
        Form {
            title: form.title,
            fields: form
                .fields
                .into_iter()
                .map(|field| Field {
                    id: field.id,
                    label: field.label,
                    kind: match field.kind {
                        WireFieldKind::Text { placeholder } => FieldKind::Text { placeholder },
                        WireFieldKind::Choice { choices } => FieldKind::Choice(
                            choices
                                .into_iter()
                                .map(|choice| Choice {
                                    id: choice.id,
                                    label: choice.label,
                                })
                                .collect(),
                        ),
                    },
                })
                .collect(),
            submit_label: form.submit_label,
        }
    }
}

#[derive(Deserialize)]
struct WireField {
    id: String,
    label: String,
    #[serde(flatten)]
    kind: WireFieldKind,
}

/// A field's kind, named by its `kind`, with that kind's own fields beside
/// it.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum WireFieldKind {
    Text {
        #[serde(default)]
        placeholder: Option<String>,
    },
    Choice {
        choices: Vec<WireChoice>,
    },
}

#[derive(Deserialize)]
struct WireChoice {
    id: String,
    label: String,
}

#[derive(Deserialize)]
struct WireCustomView {
    title: String,
    label: String,
    role: WireRole,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WireRole {
    ColorWell,
}

#[derive(Deserialize)]
struct WireAnswer {
    #[serde(default)]
    status: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_reads_with_its_items_actions_forms_and_views() {
        let view = read_view(
            r#"{"version": 1, "view": {"type": "list", "title": "Notes", "items": [
                {"id": "a", "title": "A", "subtitle": "first",
                 "actions": [{"title": "Open", "onAction": "open-a"}, {"onAction": "x"}]},
                {"id": "b", "title": "B", "platforms": ["windows", "plan9"],
                 "form": {"title": "F", "submitLabel": "Go", "fields": [
                    {"id": "n", "label": "Name", "kind": "text", "placeholder": "Ada"},
                    {"id": "c", "label": "Pick", "kind": "choice",
                     "choices": [{"id": "x", "label": "X"}]}]}},
                {"id": "c", "title": "C", "subtitle": null, "actions": null,
                 "customView": {"title": "Color", "label": "Color", "role": "color-well"}}
            ]}}"#,
        )
        .unwrap();

        assert_eq!(view.title, "Notes");
        assert_eq!(view.items.len(), 3);
        let a = &view.items[0];
        assert_eq!(a.subtitle.as_deref(), Some("first"));
        assert_eq!(a.action().unwrap().callback, "open-a");
        assert_eq!(a.action().unwrap().title.as_deref(), Some("Open"));
        let b = &view.items[1];
        assert_eq!(b.platforms, Some(vec![Platform::Windows]));
        assert_eq!(b.action(), None);
        let form = b.form.as_ref().unwrap();
        assert_eq!(
            form.fields[0].kind,
            FieldKind::Text {
                placeholder: Some("Ada".into())
            }
        );
        assert!(matches!(&form.fields[1].kind, FieldKind::Choice(choices) if choices.len() == 1));
        let c = &view.items[2];
        assert_eq!(c.actions, []);
        assert_eq!(
            c.custom_view.as_ref().map(|view| view.role),
            Some(CustomViewRole::ColorWell)
        );
    }

    #[test]
    fn unknown_fields_and_newer_versions_are_read_for_what_pane_knows() {
        let view = read_view(
            r#"{"version": 7, "renderAfter": 5, "view": {"type": "list", "title": "T",
                "dropdown": {}, "items": [{"id": "a", "title": "A", "icon": "star",
                "actions": [{"onAction": "a", "shortcut": "ctrl+o"}]}]}}"#,
        )
        .unwrap();

        assert_eq!(view.items[0].action().unwrap().callback, "a");
    }

    #[test]
    fn an_items_icon_tooltips_and_accessories_are_read_and_what_pane_cannot_draw_left_out() {
        use crate::icons::{Color, IconSource, Tone};
        let view = read_view(
            r##"{"version": 1, "view": {"type": "list", "title": "T", "items": [
                {"id": "a", "title": "A", "icon": {"builtin": "star", "tint": "red"},
                 "titleTooltip": "All of A", "subtitleTooltip": "More",
                 "accessories": [
                    {"text": "3", "tooltip": "Unread"},
                    {"date": 1767225600000, "icon": "bell"},
                    {"tag": "Open", "color": "#2f9e44"},
                    {"icon": "star"},
                    {"nothing": true},
                    {"tag": "Odd", "color": "plaid"},
                    "not an accessory"
                 ],
                 "actions": [{"onAction": "a", "icon": "copy"}, {"onAction": "b"}]},
                {"id": "b", "title": "B", "icon": {"tint": "red"}}
            ]}}"##,
        )
        .unwrap();
        let look = &view.items[0].look;
        assert_eq!(
            look.icon.as_ref().map(|icon| (&icon.source, icon.tint)),
            Some((
                &IconSource::Builtin {
                    name: "star".into(),
                    filled: false
                },
                Some(Tint::Same(Color::Tone(Tone::Red)))
            ))
        );
        assert_eq!(look.title_tooltip.as_deref(), Some("All of A"));
        assert_eq!(look.subtitle_tooltip.as_deref(), Some("More"));
        let contents: Vec<&AccessoryContent> =
            look.accessories.iter().map(|a| &a.content).collect();
        assert_eq!(
            contents,
            [
                &AccessoryContent::Text("3".into()),
                &AccessoryContent::Date(1_767_225_600_000),
                &AccessoryContent::Tag("Open".into()),
                &AccessoryContent::Text(String::new()),
                &AccessoryContent::Tag("Odd".into()),
            ]
        );
        assert_eq!(look.accessories[0].tooltip.as_deref(), Some("Unread"));
        assert!(look.accessories[1].icon.is_some());
        assert_eq!(
            look.accessories[2].color,
            Some(Tint::Same(Color::Rgba(0x2F9E44FF)))
        );
        assert_eq!(look.accessories[4].color, None, "a colour Pane cannot read");
        assert_eq!(look.action_icons.len(), 2);
        assert!(look.action_icons[0].is_some() && look.action_icons[1].is_none());
        // An icon with nothing to draw: the item has none.
        assert_eq!(view.items[1].look, ItemLook::default());
    }

    #[test]
    fn a_tree_pane_cannot_read_says_why() {
        for (tree, why) in [
            ("not json", "its view: expected"),
            (r#"{"view": {"type": "list"}}"#, "missing field `version`"),
            (r#"{"version": 0, "view": {}}"#, "names version 0"),
            (r#"{"version": 1, "view": {"title": "T"}}"#, "has no `type`"),
            (
                r#"{"version": 2, "view": {"type": "grid"}}"#,
                "a `grid` view, which this version of Pane cannot show",
            ),
            (
                r#"{"version": 1, "view": {"type": "list", "title": "T", "items": [{"id": "a"}]}}"#,
                "missing field `title`",
            ),
            (
                r#"{"version": 1, "view": {"type": "list", "title": 5, "items": []}}"#,
                "invalid type",
            ),
            (
                r#"{"version": 1, "view": {"type": "list", "title": "T", "items": [
                    {"id": "a", "title": "A", "actions": [{"title": "no callback"}]}]}}"#,
                "missing field `onAction`",
            ),
        ] {
            let error = read_view(tree).unwrap_err();
            assert!(error.contains(why), "{tree}: {error}");
        }
    }

    #[test]
    fn an_answer_reads_its_status() {
        assert_eq!(
            read_answer(r#"{"status": "Saved", "toast": {}}"#),
            Ok(Answer {
                status: Some("Saved".into())
            })
        );
        assert_eq!(read_answer("{}"), Ok(Answer::default()));
        assert!(read_answer("\"Saved\"").is_err());
    }
}
