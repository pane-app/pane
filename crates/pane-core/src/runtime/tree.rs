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

use serde::Deserialize;
use serde_json::Value;

use super::{Choice, CustomViewInfo, CustomViewRole, Field, FieldKind, Form};
use crate::keyboard::Binding;
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
    /// What the item offers, in order: the first is its primary action
    /// (Enter), the second its secondary action (Ctrl+Enter), and the
    /// Actions panel lists them all (#137).
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
}

impl Item {
    /// The action Enter runs: the first, if the item has any.
    pub fn action(&self) -> Option<&Action> {
        self.actions.first()
    }
}

/// An action of an item, or an entry of an action's submenu, which is an
/// action too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    /// What the action is called, as the footer and the Actions panel name
    /// it; `None` when the tree gives no title.
    pub title: Option<String>,
    /// What choosing it does: hand a callback id to the command, or open a
    /// submenu (#140).
    pub kind: ActionKind,
    /// The title of the section the action belongs to in the Actions
    /// panel (or in its submenu); `None` for an untitled one. Consecutive
    /// actions with the same section are one section.
    pub section: Option<String>,
    /// How the action is drawn.
    pub style: ActionStyle,
    /// The action's own shortcut on this system, as the tree gives it:
    /// `None` when it has none here, `Err` with why when the tree's
    /// shortcut is not one Pane can bind. Whether Pane binds it also
    /// depends on Pane's own keys (see `crate::keyboard::PaneKeys`).
    pub shortcut: Option<Result<Binding, String>>,
}

impl Action {
    /// The callback id Pane passes to the command's `handle-event` when the
    /// user chooses the action; `None` for one that opens a submenu.
    pub fn callback(&self) -> Option<&str> {
        match &self.kind {
            ActionKind::Callback(callback) => Some(callback),
            ActionKind::Submenu(_) => None,
        }
    }

    /// The submenu the action opens; `None` for one with a callback.
    pub fn submenu(&self) -> Option<&ActionSubmenu> {
        match &self.kind {
            ActionKind::Callback(_) => None,
            ActionKind::Submenu(submenu) => Some(submenu),
        }
    }
}

/// What choosing an action does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionKind {
    /// Pane passes this callback id to the command's `handle-event`.
    Callback(String),
    /// The Actions panel opens this submenu in place (#140).
    Submenu(ActionSubmenu),
}

/// A submenu an action opens in the Actions panel: further choices, such
/// as "Open With…", that are not all listed at once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionSubmenu {
    /// Its title, which the panel shows as its context while it is open.
    pub title: String,
    /// Its entries, or how Pane asks for them.
    pub entries: SubmenuEntries,
}

/// Where a submenu's entries come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmenuEntries {
    /// Given with the tree, in order. Each is an action: a callback, or a
    /// submenu of its own.
    Given(Vec<Action>),
    /// Asked for each time the submenu opens: Pane passes this callback id
    /// to the command's `handle-event`, and its answer's `entries` are the
    /// submenu's.
    Asked(String),
}

/// How an action is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActionStyle {
    /// As any other action.
    #[default]
    Default,
    /// In the destructive style: it deletes or removes something.
    Destructive,
}

/// What a command's `handle-event` answered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    /// Text shown to the user as the result, in the status line; `None`
    /// when the command shows none. Transitional: toasts replace it.
    pub status: Option<String>,
    /// The entries of the submenu the command was asked to open (#140), in
    /// order; `None` when the answer gives none.
    pub entries: Option<Vec<Action>>,
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
    let items = list
        .items
        .into_iter()
        .map(item)
        .collect::<Result<Vec<Item>, String>>()
        .map_err(|error| format!("its list: {error}"))?;
    Ok(View {
        title: list.title,
        items,
    })
}

/// What `answer`, the text a command's `handle-event` answered, says, or
/// why Pane cannot read it.
pub(crate) fn read_answer(answer: &str) -> Result<Answer, String> {
    let answer: WireAnswer =
        serde_json::from_str(answer).map_err(|error| format!("its answer: {error}"))?;
    let entries = answer
        .entries
        .map(|entries| {
            entries
                .into_iter()
                .map(action)
                .collect::<Result<Vec<Action>, String>>()
        })
        .transpose()
        .map_err(|error| format!("its answer: {error}"))?;
    Ok(Answer {
        status: answer.status,
        entries,
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
}

/// The item `item` describes, or why Pane cannot read it.
fn item(item: WireItem) -> Result<Item, String> {
    let actions = item
        .actions
        .unwrap_or_default()
        .into_iter()
        .map(action)
        .collect::<Result<Vec<Action>, String>>()?;
    Ok(Item {
        id: item.id,
        title: item.title,
        subtitle: item.subtitle,
        actions,
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
    })
}

/// An action, or an entry of a submenu: exactly one of `onAction` and
/// `submenu`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireAction {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    on_action: Option<String>,
    #[serde(default)]
    submenu: Option<WireSubmenu>,
    #[serde(default)]
    section: Option<String>,
    /// `default` or `destructive`; a style Pane does not know is drawn as
    /// the default.
    #[serde(default)]
    style: Option<String>,
    /// Read by [`shortcut_here`], so that a shortcut Pane cannot bind
    /// leaves the action in the panel instead of making the tree
    /// unreadable.
    #[serde(default)]
    shortcut: Option<Value>,
}

/// A submenu: its title, and exactly one of `entries` (given at once) and
/// `onOpen` (the callback id Pane passes to `handle-event` to ask for them).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireSubmenu {
    title: String,
    #[serde(default)]
    entries: Option<Vec<WireAction>>,
    #[serde(default)]
    on_open: Option<String>,
}

/// The action `action` describes, or why Pane cannot read it.
fn action(action: WireAction) -> Result<Action, String> {
    let named = match &action.title {
        Some(title) => format!("the action “{title}”"),
        None => "an untitled action".to_owned(),
    };
    let kind = match (action.on_action, action.submenu) {
        (Some(callback), None) => ActionKind::Callback(callback),
        (None, Some(submenu)) => ActionKind::Submenu(self::submenu(submenu)?),
        (Some(_), Some(_)) => {
            return Err(format!(
                "{named} has both an `onAction` and a `submenu`; an action has one of them"
            ));
        }
        (None, None) => {
            return Err(format!("{named} needs an `onAction` or a `submenu`"));
        }
    };
    Ok(Action {
        title: action.title,
        kind,
        section: action.section,
        style: match action.style.as_deref() {
            Some("destructive") => ActionStyle::Destructive,
            _ => ActionStyle::Default,
        },
        shortcut: action
            .shortcut
            .as_ref()
            .and_then(|shortcut| shortcut_here(shortcut, Platform::current())),
    })
}

/// The submenu `submenu` describes, or why Pane cannot read it.
fn submenu(submenu: WireSubmenu) -> Result<ActionSubmenu, String> {
    let entries = match (submenu.entries, submenu.on_open) {
        (Some(entries), None) => SubmenuEntries::Given(
            entries
                .into_iter()
                .map(action)
                .collect::<Result<Vec<Action>, String>>()?,
        ),
        (None, Some(callback)) => SubmenuEntries::Asked(callback),
        (Some(_), Some(_)) => {
            return Err(format!(
                "the submenu “{}” has both `entries` and an `onOpen`; a submenu has one of them",
                submenu.title
            ));
        }
        (None, None) => {
            return Err(format!(
                "the submenu “{}” needs `entries` or an `onOpen`",
                submenu.title
            ));
        }
    };
    Ok(ActionSubmenu {
        title: submenu.title,
        entries,
    })
}

/// The systems a per-system shortcut may name.
const SHORTCUT_SYSTEMS: [(&str, Platform); 3] = [
    ("windows", Platform::Windows),
    ("macos", Platform::Macos),
    ("linux", Platform::Linux),
];

/// The binding `shortcut` gives on `here`, the system Pane runs on: a
/// shortcut is one key with its modifiers for every system (`{"modifiers":
/// ["ctrl"], "key": "o"}`), or one per system (`{"windows": …, "macos": …,
/// "linux": …}`, each such a key and modifiers, any of them omitted).
/// `None` when it gives none for `here`; `Err` with why when it is not one
/// Pane can bind.
fn shortcut_here(shortcut: &Value, here: Option<Platform>) -> Option<Result<Binding, String>> {
    let Value::Object(fields) = shortcut else {
        return Some(Err("its shortcut is not an object".into()));
    };
    if fields.contains_key("key") {
        return Some(keys(shortcut));
    }
    if !SHORTCUT_SYSTEMS
        .iter()
        .any(|(name, _)| fields.contains_key(*name))
    {
        return Some(Err(
            "its shortcut names neither a key nor a system's key".into()
        ));
    }
    let (name, _) = SHORTCUT_SYSTEMS
        .iter()
        .find(|(_, system)| Some(*system) == here)?;
    match fields.get(*name) {
        None | Some(Value::Null) => None,
        Some(keys_here) => Some(self::keys(keys_here)),
    }
}

/// The binding of `{"modifiers": [...], "key": "..."}`. Modifiers are
/// `ctrl`, `alt`, `shift` and `cmd` (the Command key on macOS, the Windows
/// key on Windows, Super on Linux), with their synonyms `control`,
/// `option`, `opt`, `command`, `win`, `super` and `meta`.
fn keys(value: &Value) -> Result<Binding, String> {
    #[derive(Deserialize)]
    struct Keys {
        #[serde(default)]
        modifiers: Vec<String>,
        key: String,
    }
    let keys: Keys =
        serde_json::from_value(value.clone()).map_err(|error| format!("its shortcut: {error}"))?;
    let (mut control, mut alt, mut shift, mut platform) = (false, false, false, false);
    for modifier in &keys.modifiers {
        match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => control = true,
            "alt" | "option" | "opt" => alt = true,
            "shift" => shift = true,
            "cmd" | "command" | "win" | "super" | "meta" => platform = true,
            other => return Err(format!("its shortcut names an unknown modifier “{other}”")),
        }
    }
    Binding::new(control, alt, shift, platform, false, &keys.key)
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
    /// A submenu's entries, when the command was asked to open one.
    #[serde(default)]
    entries: Option<Vec<WireAction>>,
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
        assert_eq!(a.action().unwrap().callback(), Some("open-a"));
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

        assert_eq!(view.items[0].action().unwrap().callback(), Some("a"));
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
                "the action “no callback” needs an `onAction` or a `submenu`",
            ),
        ] {
            let error = read_view(tree).unwrap_err();
            assert!(error.contains(why), "{tree}: {error}");
        }
    }

    #[test]
    fn actions_read_their_sections_styles_and_shortcuts() {
        let view = read_view(
            r#"{"version": 1, "view": {"type": "list", "title": "T", "items": [
                {"id": "a", "title": "A", "actions": [
                    {"title": "Open", "onAction": "a"},
                    {"title": "Copy Link", "onAction": "a#1", "section": "Share",
                     "shortcut": {"modifiers": ["ctrl", "shift"], "key": "C"}},
                    {"title": "Delete", "onAction": "a#2", "style": "destructive",
                     "section": null, "shortcut": null},
                    {"title": "Glow", "onAction": "a#3", "style": "glowing",
                     "shortcut": {"modifiers": ["hyper"], "key": "g"}}
                ]}]}}"#,
        )
        .unwrap();

        let actions = &view.items[0].actions;
        assert_eq!(actions[0].section, None);
        assert_eq!(actions[0].style, ActionStyle::Default);
        assert_eq!(actions[0].shortcut, None);
        assert_eq!(actions[1].section.as_deref(), Some("Share"));
        assert_eq!(
            actions[1].shortcut,
            Some(Ok(Binding::parse("ctrl-shift-c").unwrap()))
        );
        assert_eq!(actions[2].style, ActionStyle::Destructive);
        assert_eq!(actions[2].shortcut, None);
        // An unknown style is the default; a shortcut Pane cannot bind
        // says why, and the action stays.
        assert_eq!(actions[3].style, ActionStyle::Default);
        assert!(matches!(&actions[3].shortcut, Some(Err(why)) if why.contains("hyper")));
    }

    #[test]
    fn a_shortcut_per_system_binds_only_on_its_system() {
        let shortcut = serde_json::json!({
            "windows": {"modifiers": ["ctrl", "shift"], "key": "e"},
            "macos": {"modifiers": ["cmd", "shift"], "key": "r"},
        });
        let here = |platform| shortcut_here(&shortcut, Some(platform));
        assert_eq!(
            here(Platform::Windows),
            Some(Ok(Binding::parse("ctrl-shift-e").unwrap()))
        );
        assert_eq!(
            here(Platform::Macos),
            Some(Ok(Binding::parse("cmd-shift-r").unwrap()))
        );
        assert_eq!(here(Platform::Linux), None, "none for Linux");

        let everywhere = serde_json::json!({"modifiers": ["ctrl"], "key": "d"});
        for platform in [Platform::Windows, Platform::Macos, Platform::Linux] {
            assert_eq!(
                shortcut_here(&everywhere, Some(platform)),
                Some(Ok(Binding::parse("ctrl-d").unwrap()))
            );
        }

        for (wrong, why) in [
            (serde_json::json!("ctrl-d"), "not an object"),
            (serde_json::json!({"keys": "d"}), "neither a key"),
            (
                serde_json::json!({"key": "d", "modifiers": "ctrl"}),
                "invalid type",
            ),
            (serde_json::json!({"key": "ctrl"}), "hold one more key"),
        ] {
            let read = shortcut_here(&wrong, Some(Platform::Linux));
            assert!(
                matches!(&read, Some(Err(error)) if error.contains(why)),
                "{wrong}: {read:?}"
            );
        }
    }

    #[test]
    fn an_answer_reads_its_status() {
        assert_eq!(
            read_answer(r#"{"status": "Saved", "toast": {}}"#),
            Ok(Answer {
                status: Some("Saved".into()),
                entries: None,
            })
        );
        assert_eq!(read_answer("{}"), Ok(Answer::default()));
        assert!(read_answer("\"Saved\"").is_err());
    }

    #[test]
    fn a_submenu_is_given_at_once_or_asked_for_when_opened() {
        let view = read_view(
            r#"{"version": 1, "view": {"type": "list", "title": "T", "items": [
                {"id": "a", "title": "A", "actions": [
                    {"title": "Open", "onAction": "a"},
                    {"title": "Open With…", "section": "Share",
                     "submenu": {"title": "Open With", "entries": [
                        {"title": "Notepad", "onAction": "a#1/0", "section": "Editors",
                         "shortcut": {"modifiers": ["ctrl", "shift"], "key": "n"}},
                        {"title": "More", "submenu": {"title": "More", "onOpen": "a#1/1"}},
                        {"title": "Forget", "onAction": "a#1/2", "style": "destructive"}
                     ]}},
                    {"title": "Move to List…",
                     "submenu": {"title": "Move to List", "onOpen": "a#2", "icon": "list"}}
                ]}]}}"#,
        )
        .unwrap();

        let actions = &view.items[0].actions;
        assert_eq!(actions[0].callback(), Some("a"));
        assert_eq!(actions[0].submenu(), None);
        assert_eq!(actions[1].callback(), None);
        assert_eq!(actions[1].section.as_deref(), Some("Share"));
        let open_with = actions[1].submenu().unwrap();
        assert_eq!(open_with.title, "Open With");
        let SubmenuEntries::Given(entries) = &open_with.entries else {
            panic!("given at once: {open_with:?}");
        };
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].callback(), Some("a#1/0"));
        assert_eq!(entries[0].section.as_deref(), Some("Editors"));
        assert_eq!(
            entries[0].shortcut,
            Some(Ok(Binding::parse("ctrl-shift-n").unwrap()))
        );
        // An entry may open a submenu of its own.
        assert_eq!(
            entries[1].submenu().map(|more| &more.entries),
            Some(&SubmenuEntries::Asked("a#1/1".into()))
        );
        assert_eq!(entries[2].style, ActionStyle::Destructive);
        let lists = actions[2].submenu().unwrap();
        assert_eq!(
            (lists.title.as_str(), &lists.entries),
            ("Move to List", &SubmenuEntries::Asked("a#2".into()))
        );
    }

    #[test]
    fn a_submenu_pane_cannot_read_says_why() {
        let tree = |action: &str| {
            format!(
                r#"{{"version": 1, "view": {{"type": "list", "title": "T", "items": [
                    {{"id": "a", "title": "A", "actions": [{action}]}}]}}}}"#
            )
        };
        for (action, why) in [
            (
                r#"{"title": "Both", "onAction": "a", "submenu": {"title": "S", "onOpen": "s"}}"#,
                "the action “Both” has both an `onAction` and a `submenu`",
            ),
            (
                r#"{"submenu": {"title": "S", "entries": [], "onOpen": "s"}}"#,
                "the submenu “S” has both `entries` and an `onOpen`",
            ),
            (
                r#"{"submenu": {"title": "S"}}"#,
                "the submenu “S” needs `entries` or an `onOpen`",
            ),
            (r#"{"submenu": {"onOpen": "s"}}"#, "missing field `title`"),
            (
                r#"{"title": "Outer", "submenu": {"title": "S", "entries": [{"title": "Inner"}]}}"#,
                "the action “Inner” needs an `onAction` or a `submenu`",
            ),
        ] {
            let error = read_view(&tree(action)).unwrap_err();
            assert!(error.contains(why), "{action}: {error}");
        }
    }

    #[test]
    fn an_answer_reads_a_submenus_entries() {
        let answer = read_answer(
            r#"{"entries": [
                {"title": "Inbox", "onAction": "m/0", "section": "Lists"},
                {"title": "Nested", "submenu": {"title": "N", "entries": []}}
            ]}"#,
        )
        .unwrap();
        assert_eq!(answer.status, None);
        let entries = answer.entries.unwrap();
        assert_eq!(entries[0].callback(), Some("m/0"));
        assert_eq!(entries[0].section.as_deref(), Some("Lists"));
        assert_eq!(
            entries[1].submenu().map(|nested| &nested.entries),
            Some(&SubmenuEntries::Given(Vec::new()))
        );

        assert_eq!(
            read_answer(r#"{"entries": []}"#).unwrap().entries,
            Some(Vec::new())
        );
        let error = read_answer(r#"{"entries": [{"title": "X"}]}"#).unwrap_err();
        assert!(error.starts_with("its answer: "), "{error}");
        assert!(error.contains("needs an `onAction`"), "{error}");
    }
}
