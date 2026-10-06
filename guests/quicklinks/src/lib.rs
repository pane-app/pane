//! Pane's quicklinks, a default extension: the user saves a named web
//! address through its command's form, and typing the name (or part of the
//! address) into root search lists it; invoking it opens the address with
//! the system's handler for web links. Quicklinks are kept in the package's
//! content, its durable extension data, so they survive restarts, updates,
//! disabling and clearing its cache (see [`links`]).
#![no_std]

mod links;

use pane_guest::alloc::{format, string::String, vec, vec::Vec};
use pane_guest::root::{RootAction, RootResult};
use pane_guest::{
    Choice, Command, CustomView, Field, FieldKind, FieldValue, Form, FormError, Item, List,
    NoCustomView, TextField,
};

use links::Quicklink;

struct Quicklinks;
pane_guest::export!(Quicklinks);
pane_guest::root::export!(Quicklinks);

/// The item that creates a quicklink.
const CREATE: &str = "create";
/// The prefix of the item that edits the quicklink named by the rest.
const EDIT: &str = "edit:";
/// The edit form's choice between saving and removing.
const THEN: &str = "then";

fn text(id: &str, label: &str, placeholder: &str) -> Field {
    Field {
        id: id.into(),
        label: label.into(),
        kind: FieldKind::Text(TextField {
            placeholder: Some(placeholder.into()),
        }),
    }
}

fn create_form() -> Form {
    Form {
        title: "Create quicklink".into(),
        fields: vec![
            text("name", "Name", "Pane issues"),
            text("url", "URL", "https://github.com/hoangvu12/pane/issues"),
        ],
        submit_label: "Save quicklink".into(),
    }
}

/// Edits `link`: an empty field keeps its value, which it shows while empty.
fn edit_form(link: &Quicklink) -> Form {
    let choice = |id: &str, label: &str| Choice {
        id: id.into(),
        label: label.into(),
    };
    Form {
        title: format!("Edit “{}”", link.name),
        fields: vec![
            text("name", "Name", &format!("{} (unchanged)", link.name)),
            text("url", "URL", &format!("{} (unchanged)", link.url)),
            Field {
                id: THEN.into(),
                label: "On submit".into(),
                kind: FieldKind::Choice(vec![
                    choice("save", "Save changes"),
                    choice("remove", "Remove this quicklink"),
                ]),
            },
        ],
        submit_label: "Apply".into(),
    }
}

fn value<'a>(values: &'a [FieldValue], id: &str) -> &'a str {
    values
        .iter()
        .find(|value| value.id == id)
        .map_or("", |value| value.value.trim())
}

fn field_error(field: &str, message: String) -> FormError {
    FormError {
        field: Some(field.into()),
        message,
    }
}

fn form_error(message: String) -> FormError {
    FormError {
        field: None,
        message,
    }
}

/// Checks `name` and `url` for the quicklink at `index` (a new one when
/// `None`) among `saved`.
fn check(
    saved: &[Quicklink],
    index: Option<usize>,
    name: &str,
    url: &str,
) -> Result<(), FormError> {
    if let Some(problem) = links::name_problem(name) {
        return Err(field_error("name", problem));
    }
    if let Some(other) = links::find(saved, name).filter(|&other| Some(other) != index) {
        let existing = &saved[other].name;
        return Err(field_error(
            "name",
            format!("A quicklink named “{existing}” already exists"),
        ));
    }
    if let Some(problem) = links::url_problem(url) {
        return Err(field_error("url", problem));
    }
    Ok(())
}

impl Command for Quicklinks {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let create = Item::new(CREATE, "Create quicklink")
            .subtitle("Name a web address to open from root search")
            .form(create_form());
        let saved = links::load()?;
        let edits = saved.iter().map(|link| {
            Item::new(format!("{EDIT}{}", link.name), link.name.clone())
                .subtitle(format!("{} · Enter edits or removes it", link.url))
                .form(edit_form(link))
        });
        Ok(List::new("Quicklinks").item(create).items(edits))
    }

    async fn submit_form(item_id: String, values: Vec<FieldValue>) -> Result<String, FormError> {
        let mut saved = links::load().map_err(form_error)?;
        let (name, url) = (value(&values, "name"), value(&values, "url"));
        let message = if item_id == CREATE {
            check(&saved, None, name, url)?;
            saved.push(Quicklink {
                name: name.into(),
                url: url.into(),
            });
            format!("Saved quicklink “{name}”")
        } else if let Some(current) = item_id.strip_prefix(EDIT) {
            let index = saved
                .iter()
                .position(|link| link.name == current)
                .ok_or_else(|| form_error(format!("“{current}” no longer exists")))?;
            if value(&values, THEN) == "remove" {
                saved.remove(index);
                format!("Removed quicklink “{current}”")
            } else {
                let link = &saved[index];
                let name = if name.is_empty() { &link.name } else { name };
                let url = if url.is_empty() { &link.url } else { url };
                check(&saved, Some(index), name, url)?;
                let message = format!("Saved quicklink “{name}”");
                saved[index] = Quicklink {
                    name: name.into(),
                    url: url.into(),
                };
                message
            }
        } else {
            return Err(form_error(format!("unknown form: {item_id}")));
        };
        links::save(&saved).map_err(form_error)?;
        Ok(message)
    }

    async fn open_view(item_id: String) -> Result<CustomView, String> {
        Err(format!("unknown view: {item_id}"))
    }
}

impl pane_guest::root::Guest for Quicklinks {
    /// The saved quicklinks the query finds, each opening its address.
    async fn results_for(query: String) -> Result<Vec<RootResult>, String> {
        let saved = links::load()?;
        Ok(links::matching(&saved, &query)
            .into_iter()
            .map(|link| RootResult {
                id: link.name.to_lowercase(),
                title: link.name.clone(),
                subtitle: Some(format!("Quicklink · {}", link.url)),
                action: RootAction::OpenUrl(link.url.clone()),
            })
            .collect())
    }
}
