//! The `__NAME__` command: an item that opens a form the command answers,
//! as the `form` template's own command does. Added to the package by
//! `pane-ext new command`; the entry file (`src/lib.rs`) calls its
//! `render` and `submit_form`.

use pane_extension::alloc::{format, string::String, vec, vec::Vec};
use pane_extension::{
    Choice, Field, FieldKind, FieldValue, Form, FormError, Item, List, TextField,
};

/// The greeting form's choices, by id.
const GREETINGS: [(&str, &str); 3] = [
    ("hello", "Hello"),
    ("morning", "Good morning"),
    ("welcome", "Welcome"),
];

/// The command's list: the form's item. Choosing it opens the form;
/// submitting it calls `submit_form` with the item's id.
pub(crate) async fn render() -> Result<List, String> {
    Ok(List::new("__TITLE__").item(
        Item::new("greet", "Greet someone")
            .subtitle("Fill in a form the command checks and answers")
            .form(greeting_form()),
    ))
}

/// The form's answer: the chosen greeting and the name, or the problem
/// with a field, shown next to it.
pub(crate) async fn submit_form(
    item_id: String,
    values: Vec<FieldValue>,
) -> Result<String, FormError> {
    if item_id != "greet" {
        return Err(FormError {
            field: None,
            message: format!("unknown form: {item_id}"),
        });
    }
    let value = |id: &str| {
        values
            .iter()
            .find(|field| field.id == id)
            .map_or("", |field| field.value.as_str())
    };
    let name = value("name").trim();
    if name.is_empty() {
        return Err(FormError {
            field: Some("name".into()),
            message: "Enter a name".into(),
        });
    }
    if name.chars().count() > 40 {
        return Err(FormError {
            field: Some("name".into()),
            message: "Use at most 40 characters".into(),
        });
    }
    let Some(&(_, greeting)) = GREETINGS.iter().find(|&&(id, _)| id == value("greeting")) else {
        return Err(FormError {
            field: Some("greeting".into()),
            message: "Choose a greeting".into(),
        });
    };
    Ok(format!("{greeting}, {name}"))
}

/// The item's form: a name to type and a greeting to choose.
fn greeting_form() -> Form {
    Form {
        title: "Greet someone".into(),
        fields: vec![
            Field {
                id: "name".into(),
                label: "Name".into(),
                kind: FieldKind::Text(TextField {
                    placeholder: Some("Ada Lovelace".into()),
                }),
            },
            Field {
                id: "greeting".into(),
                label: "Greeting".into(),
                kind: FieldKind::Choice(
                    GREETINGS
                        .iter()
                        .map(|&(id, label)| Choice {
                            id: id.into(),
                            label: label.into(),
                        })
                        .collect(),
                ),
            },
        ],
        submit_label: "Greet".into(),
    }
}
