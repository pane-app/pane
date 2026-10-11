//! The form a designed tree holds (#241): a `form` node whose children
//! are the author's own layout and whose submission is an action, and
//! the field components — the date and date-time pickers, the tag
//! picker, the file and folder pickers — with the chrome every field
//! shares: its title over its control, its note and the error the
//! extension's last answer set under it. The text fields, dropdowns,
//! checkboxes and toggles are the components they always were
//! (`components`), wearing the same chrome through
//! [`field_group`].
//!
//! Submission collects the form's fields' values from the state the
//! window keeps for them — the text a field edits, the tags a picker
//! chose — and runs the form's `onSubmit` with them; a form Pane itself
//! asks (the tree's `form` node naming no `onSubmit`) hands its values
//! to the launcher instead. Enter in a single-line field submits, and
//! Ctrl+Enter does from a text area, whose Enter inserts a newline; the
//! form's submit button does the same.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyElement, ClickEvent, Role, SharedString, div, px};

use pane_core::{
    DateField as DateFieldNode, FieldProps, FilePicker as FilePickerNode, FormNode, Node,
    TagPicker as TagPickerNode, TextInput as TextInputNode,
};

use crate::app::LauncherWindow;
use crate::ui::controls;
use crate::ui::theme::Theme;
use crate::ui::tokens;

/// The callback a path picker's "Choose…" button runs: it opens the
/// system dialog for the pick and fills the field with what it returns.
type Choose =
    Rc<dyn Fn(&mut LauncherWindow, &mut gpui::Window, &mut gpui::Context<LauncherWindow>)>;

use super::components::{self, FieldKind};
use super::reconcile::Held;
use super::tree::Draw;
use super::{BUTTON_CONTEXT, DATE_CONTEXT, PATHS_CONTEXT, Press, TAGS_CONTEXT};

/// One field's chrome: its title over `control`, and the note and error
/// under it — the Settings board's field family, as the typed form's
/// fields wore (#99). `key` is the field's key, `label` what names the
/// control to assistive technology when the title does not.
pub(super) fn field_group(
    key: &str,
    props: &FieldProps,
    label: Option<&str>,
    control: impl IntoElement,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let label_selector = format!("field-label-{key}");
    let error_selector = format!("field-error-{key}");
    let info_selector = format!("field-description-{key}");
    controls::field(theme)
        .id(SharedString::from(format!("field-{key}")))
        .debug_selector(move || format!("field-{key}"))
        .when_some(
            props.title.clone().or_else(|| label.map(str::to_owned)),
            |group, title| {
                group.child(
                    controls::field_label(title, theme)
                        .debug_selector(move || label_selector.clone()),
                )
            },
        )
        .child(control)
        .when_some(props.error.clone(), |group, error| {
            group.child(
                controls::field_description(error, theme.danger, theme)
                    .debug_selector(move || error_selector.clone()),
            )
        })
        .when_some(props.info.clone(), |group, info| {
            group.child(
                controls::field_description(info, theme.text_muted, theme)
                    .debug_selector(move || info_selector.clone()),
            )
        })
}

/// The label a field names itself by to assistive technology: its title,
/// else the label its kind carries, else a plain word.
pub(super) fn field_label(props: &FieldProps, label: Option<&str>) -> SharedString {
    props
        .title
        .clone()
        .or_else(|| label.map(str::to_owned))
        .unwrap_or_else(|| "field".into())
        .into()
}

/// The placeholder a date field shows: the format its value takes.
const DATE_PLACEHOLDER: &str = "2026-01-31";
const DATE_TIME_PLACEHOLDER: &str = "2026-01-31 14:05";

/// One date or date and time field: a text field whose value is the date
/// or date and time as it is written, typed or stepped with the arrow
/// keys — Up and Down step a date by a day, a date and time by a minute,
/// the whole text rewritten and its caret at the end. The extension
/// validates what it parses, as a text field's value; a calendar popover
/// is a later polish.
pub(super) fn date_field(
    node: &Node,
    field: &DateFieldNode,
    path: &str,
    draw: &Draw,
    date_time: bool,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let Some((editing, focus, _)) = draw.field(path) else {
        return div().into_any_element();
    };
    let placeholder = if date_time {
        DATE_TIME_PLACEHOLDER
    } else {
        DATE_PLACEHOLDER
    };
    let label = field_label(&field.field, None);
    let control = components::field_well(
        &TextInputNode {
            value: String::new(),
            placeholder: Some(placeholder.into()),
            label: Some(label.as_ref().to_owned()),
            field: field.field.clone(),
            ..TextInputNode::default()
        },
        editing,
        focus,
        path,
        FieldKind::Text,
        theme,
        cx,
    );
    // Up and Down step the date; the field's other keys are the text
    // field's own.
    let up_path = path.to_owned();
    let up = cx.listener(move |this, _: &super::StepDateUp, window, cx| {
        this.designed_date_stepped(&up_path, 1, window, cx);
    });
    let down_path = path.to_owned();
    let down = cx.listener(move |this, _: &super::StepDateDown, window, cx| {
        this.designed_date_stepped(&down_path, -1, window, cx);
    });
    field_group(
        node.key.as_deref().unwrap_or_default(),
        &field.field,
        None,
        div()
            .key_context(DATE_CONTEXT)
            .on_action(up)
            .on_action(down)
            .child(control),
        theme,
    )
    .into_any_element()
}

/// One tag picker: a field well of the chosen tags as chips, each
/// removable, with the query typed inline — the matching options listed
/// under the well, the highlighted one Enter commits (a click on one
/// commits it too).
pub(super) fn tag_field(
    node: &Node,
    picker: &TagPickerNode,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let Some(entry) = draw.state.get(path) else {
        return div().into_any_element();
    };
    let Held::Tags {
        query,
        focus,
        chosen,
        highlighted,
    } = &entry.held
    else {
        return div().into_any_element();
    };
    let label = field_label(&picker.field, None);
    let text = query.read(cx).as_str().to_owned();
    let matches = matching(&picker.options, &text);
    let highlighted = highlighted.filter(|at| *at < matches.len()).unwrap_or(0);
    // The well holds the query's editable text and the chosen chips; the
    // field's group carries the tag picker's context, where Enter commits
    // the highlighted option.
    let well = controls::well(false, theme)
        .id(SharedString::from(format!("{path}/well")))
        .debug_selector(move || format!("field-{}", node.key.as_deref().unwrap_or_default()))
        .track_focus(focus)
        .role(Role::EditableComboBox)
        .map(|well| well.aria_label(label.clone()))
        .map(|well| {
            well.aria_value(SharedString::from(chosen.join(", ")))
                .when_some(picker.field.title.clone(), |well, title| {
                    well.aria_placeholder(title)
                })
        })
        .focus(move |well| well.shadow(controls::well_shadows(true, theme)))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(tokens::space(pane_core::Space::Xs))
        .px(tokens::space(pane_core::Space::Xs))
        .children(chosen.iter().map(|tag| {
            let (path, tag) = (path.to_owned(), tag.clone());
            let callback = picker.on_change;
            components::chosen_chip(tag.as_str(), theme).on_click(cx.listener(
                move |this, _: &ClickEvent, window, cx| {
                    this.designed_tag_removed(&path, &tag, callback, window, cx);
                },
            ))
        }))
        .child(
            div()
                .flex_1()
                .min_w(px(40.))
                .child(components::query_input(query.clone(), theme)),
        );
    let options = if text.trim().is_empty() {
        Vec::new()
    } else {
        // The options matching the query, the highlighted one first among
        // them.
        matches
            .iter()
            .enumerate()
            .map(|(at, option)| {
                let (option, at) = (option.clone(), at);
                let path = path.to_owned();
                let value = option.value.clone();
                let callback = picker.on_change;
                option_row(
                    SharedString::from(format!("{path}/{value}")),
                    &option.label.clone().unwrap_or_else(|| option.value.clone()),
                    at == highlighted,
                    theme,
                )
                .role(Role::ListBoxOption)
                .aria_label(option.label.clone().unwrap_or_else(|| option.value.clone()))
                .when(at == highlighted, |row| row.aria_active_descendant())
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.designed_tag_added(&path, &value, callback, window, cx);
                }))
                .into_any_element()
            })
            .collect::<Vec<AnyElement>>()
    };
    let options = (!options.is_empty()).then(|| {
        div()
            .id(SharedString::from(format!("{path}/options")))
            .flex()
            .flex_col()
            .gap(px(2.))
            .mt(px(2.))
            .max_h(px(160.))
            .overflow_y_scroll()
            .role(Role::ListBox)
            .aria_label("Tags")
            .children(options)
    });
    // Enter commits the highlighted option.
    let commit = path.to_owned();
    let enter = cx.listener(move |this, _: &super::CommitTag, window, cx| {
        this.designed_tag_committed(&commit, window, cx);
    });
    field_group(
        node.key.as_deref().unwrap_or_default(),
        &picker.field,
        None,
        div()
            .key_context(TAGS_CONTEXT)
            .on_action(enter)
            .child(well)
            .children(options),
        theme,
    )
    .into_any_element()
}

/// The options matching `query`, in the order the tree lists them.
pub(super) fn matching(options: &[pane_core::Segment], query: &str) -> Vec<pane_core::Segment> {
    let query = query.trim().to_lowercase();
    options
        .iter()
        .filter(|option| {
            query.is_empty()
                || option
                    .label
                    .as_deref()
                    .unwrap_or(&option.value)
                    .to_lowercase()
                    .contains(&query)
                || option.value.to_lowercase().contains(&query)
        })
        .cloned()
        .collect()
}

/// One file or folder picker: a path typed or chosen with the system's
/// dialog — its "Choose…" button opens it, the path (or paths, for one
/// that allows many) filling the field. One path is a text field beside
/// the button; several are chips in a well beside it.
pub(super) fn path_field(
    node: &Node,
    picker: &FilePickerNode,
    path: &str,
    draw: &Draw,
    folder: bool,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    let label = field_label(&picker.field, None);
    let (field_id, pick) = (
        node.key.clone().unwrap_or_default(),
        super::PathPick {
            files: !folder,
            // A macOS application is a folder (its bundle), so the file
            // picker chooses one there too, as the Settings card's does.
            directories: folder || cfg!(target_os = "macos"),
            multiple: picker.multiple,
        },
    );
    let (well, choose): (AnyElement, Choose) = if picker.multiple {
        let Some(entry) = draw.state.get(path) else {
            return div().into_any_element();
        };
        let Held::Paths { focus, paths } = &entry.held else {
            return div().into_any_element();
        };
        let chips = paths
            .iter()
            .map(|picked| {
                let path = path.to_owned();
                let picked = picked.clone();
                components::chosen_chip(picked.as_str(), theme).on_click(cx.listener(
                    move |this, _: &ClickEvent, window, cx| {
                        this.designed_path_removed(&path, window, cx);
                    },
                ))
            })
            .collect::<Vec<_>>();
        let well_id = field_id.clone();
        let well = controls::well(false, theme)
            .id(SharedString::from(format!("{path}/well")))
            .debug_selector(move || format!("field-{well_id}"))
            .track_focus(focus)
            .role(Role::Group)
            .map(|well| well.aria_label(label.clone()))
            .map(|well| well.aria_value(SharedString::from(paths.join(", "))))
            .focus(move |well| well.shadow(controls::well_shadows(true, theme)))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(tokens::space(pane_core::Space::Xs))
            .px(tokens::space(pane_core::Space::Xs))
            .children(chips)
            .child(
                div()
                    .flex_1()
                    .min_w(px(40.))
                    .text_size(theme.typography.settings_text_size)
                    .text_color(theme.text_placeholder)
                    .child("Choose with the button"),
            );
        let (choose_path, choose_id) = (path.to_owned(), field_id.clone());
        let well = div()
            .key_context(PATHS_CONTEXT)
            .on_action(cx.listener(move |this, _: &super::ChoosePath, window, cx| {
                this.designed_paths_chosen(&choose_path, &choose_id, pick, window, cx);
            }))
            .child(well)
            .into_any_element();
        let (choose_path, choose_id) = (path.to_owned(), field_id.clone());
        (
            well,
            Rc::new(
                move |this: &mut LauncherWindow,
                      window: &mut gpui::Window,
                      cx: &mut gpui::Context<LauncherWindow>| {
                    this.designed_paths_chosen(&choose_path, &choose_id, pick, window, cx);
                },
            ),
        )
    } else {
        let Some((editing, focus, _)) = draw.field(path) else {
            return div().into_any_element();
        };
        let well = components::field_well(
            &TextInputNode {
                value: String::new(),
                placeholder: picker.field.title.clone(),
                label: Some(label.to_string()),
                field: picker.field.clone(),
                ..TextInputNode::default()
            },
            editing,
            focus,
            path,
            FieldKind::Text,
            theme,
            cx,
        )
        .flex_1();
        let (choose_path, field_id) = (path.to_owned(), field_id.clone());
        (
            well.into_any_element(),
            Rc::new(
                move |this: &mut LauncherWindow,
                      window: &mut gpui::Window,
                      cx: &mut gpui::Context<LauncherWindow>| {
                    this.designed_paths_chosen(&choose_path, &field_id, pick, window, cx);
                },
            ),
        )
    };
    let selector = format!("field-choose-{field_id}");
    let button =
        controls::ghost_button(SharedString::from(selector.clone()), "Choose…", true, theme)
            .debug_selector(move || selector.clone())
            .role(Role::Button)
            .map(|button| button.aria_label(format!("Choose {}", label)))
            .on_click(cx.listener(move |this, _, window, cx| choose(this, window, cx)));
    field_group(
        node.key.as_deref().unwrap_or_default(),
        &picker.field,
        None,
        div()
            .flex()
            .items_center()
            .gap(theme.geometry.controls.button_gap)
            .child(well)
            .child(button),
        theme,
    )
    .into_any_element()
}

/// One form: a column of its children — the author's own layout, the
/// fields anywhere in it — with the submit button under them, the form's
/// primary action. Enter in a single-line field of the form runs it, as
/// Ctrl+Enter does in a text area; the button does too, and the footer's
/// primary action with them.
pub(super) fn form(
    node: &Node,
    form: &FormNode,
    path: &str,
    draw: &Draw,
    children: Vec<AnyElement>,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    let theme = draw.theme;
    // The submit button: the form's own focus, the form's primary action.
    let focus = draw.focus_of(path);
    let submit = form.submit_label.clone().unwrap_or_else(|| "Submit".into());
    let button = controls::button("submit", submit.clone(), true, theme)
        .debug_selector(move || "submit".into())
        .role(Role::Button)
        .map(|button| button.aria_label(submit.clone()))
        .when_some(focus, |button, focus| {
            let ring = controls::focus_ring(theme);
            button
                .track_focus(&focus)
                .focus(move |button| button.shadow(ring))
        })
        // Enter and Space press the focused button, as every designed
        // button's do, both running the submission the click does.
        .key_context(BUTTON_CONTEXT)
        .on_action(cx.listener(|this, _: &Press, window, cx| {
            this.submit_designed_form(window, cx);
        }))
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            this.submit_designed_form(window, cx);
        }));
    div()
        .id(path.to_owned())
        .flex()
        .flex_col()
        .min_w(px(0.))
        .gap(tokens::space(pane_core::Space::M))
        .role(Role::Form)
        .map(|group| group.when_some(node.name.clone(), |group, name| group.aria_label(name)))
        .children(children)
        .child(div().flex().child(button))
        .into_any_element()
}

/// Steps the date `text` by `step`: a date (`2026-01-31`) by days, a
/// date and time (`2026-01-31 14:05`) by minutes — `None` when the text
/// does not parse as either.
pub(super) fn step_date(text: &str, step: i64) -> Option<String> {
    let text = text.trim();
    if let Some((date, time)) = text.split_once(' ') {
        let (year, month, day) = date_parts(date)?;
        let (hour, minute) = time_parts(time)?;
        let minutes = to_minutes(year, month, day, hour, minute)? + step;
        let (year, month, day, hour, minute) = from_minutes(minutes);
        return Some(format!(
            "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}"
        ));
    }
    let (year, month, day) = date_parts(text)?;
    let days = to_days(year, month, day)? + step;
    let (year, month, day) = from_days(days);
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// The (year, month, day) of `date` as `YYYY-MM-DD`, if it parses.
fn date_parts(date: &str) -> Option<(i64, u32, u32)> {
    let (year, rest) = date.split_once('-')?;
    let (month, day) = rest.split_once('-')?;
    let year: i64 = year.parse().ok()?;
    let month: u32 = month.parse().ok()?;
    let day: u32 = day.parse().ok()?;
    (1..=12).contains(&month).then_some((year, month, day))
}

/// The (hour, minute) of `time` as `HH:MM`, if it parses.
fn time_parts(time: &str) -> Option<(u32, u32)> {
    let (hour, minute) = time.split_once(':')?;
    let hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;
    (hour < 24 && minute < 60).then_some((hour, minute))
}

/// Days since 1970-01-01 of the date, if it is a real one.
fn to_days(year: i64, month: u32, day: u32) -> Option<i64> {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let lengths = [
        31,
        28 + u32::from(leap),
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let month = usize::try_from(month).ok()?;
    let in_month = lengths.get(month.checked_sub(1)?)?;
    (1..=*in_month).contains(&day).then(|| {
        let before: u32 = lengths[..month - 1].iter().sum();
        days_of(year) + i64::from(before + day - 1)
    })
}

/// The date of `days` since 1970-01-01.
fn from_days(days: i64) -> (i64, u32, u32) {
    let mut year = 1970;
    let mut rest = days;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let length = if leap { 366 } else { 365 };
        if rest < length {
            break;
        }
        rest -= length;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let lengths = [
        31,
        28 + u32::from(leap),
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for length in lengths {
        if rest < i64::from(length) {
            break;
        }
        rest -= i64::from(length);
        month += 1;
    }
    (year, month, rest as u32 + 1)
}

/// Days from year 1 to the start of `year`.
fn days_of(year: i64) -> i64 {
    let years = year - 1;
    let leaps = years / 4 - years / 100 + years / 400;
    years * 365 + leaps - 719_162
}

/// Minutes since midnight of 1970-01-01, if the date and time are real.
fn to_minutes(year: i64, month: u32, day: u32, hour: u32, minute: u32) -> Option<i64> {
    Some(to_days(year, month, day)? * 24 * 60 + i64::from(hour) * 60 + i64::from(minute))
}

/// The date and time of `minutes` since midnight of 1970-01-01.
fn from_minutes(minutes: i64) -> (i64, u32, u32, u32, u32) {
    let days = minutes.div_euclid(24 * 60);
    let rest = minutes.rem_euclid(24 * 60);
    let (year, month, day) = from_days(days);
    (year, month, day, (rest / 60) as u32, (rest % 60) as u32)
}

/// One option of a tag picker's list, highlighted or not.
fn option_row(
    id: SharedString,
    label: &str,
    highlighted: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .px(px(8.))
        .py(px(3.))
        .rounded(px(4.))
        .cursor_pointer()
        .text_size(theme.typography.settings_text_size)
        .text_color(theme.text_title)
        .when(highlighted, |row| {
            row.bg(theme.row_selected).text_color(theme.text_title)
        })
        .child(label.to_owned())
}
