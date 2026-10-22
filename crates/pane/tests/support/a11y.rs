//! A window's accessibility tree, as the tests read it. Shared by the
//! test binaries of `pane`.
#![allow(dead_code)]

use gpui::VisualTestContext;

/// The accessibility tree of the window `cx` drives, as raw JSON, forced
/// on so the tree is built regardless of platform accessibility.
pub fn a11y(cx: &mut VisualTestContext) -> String {
    cx.update(|window, _| window.set_a11y_forced(true));
    cx.run_until_parked();
    cx.update(|window, _| window.debug_a11y_tree_json())
        .expect("an accessibility tree")
}

/// The window's accessibility tree as (focused label, raw JSON), forced on
/// so the tree is built regardless of platform accessibility. The focused
/// label is the focused node's own, or its active descendant's.
pub fn accessibility(cx: &mut VisualTestContext) -> (Option<String>, String) {
    let json = a11y(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes = tree["nodes"].as_object().unwrap();
    let field = |node: &serde_json::Value, key: &str| {
        node["aria"][key].as_str().unwrap_or_default().to_owned()
    };
    let focused = ["active_descendant_focus", "gpui_focus"]
        .iter()
        .find_map(|key| tree[key].as_str())
        .map(|id| field(&nodes[id], "label"));
    (focused, json)
}

/// The label of the node assistive technology treats as focused in the
/// window `cx` drives.
pub fn focused_label(cx: &mut VisualTestContext) -> Option<String> {
    let (label, _) = accessibility(cx);
    label
}

/// The launcher window's announcer (#132) in the tree `json`: its one
/// `Label` node, the window's live region, as (name, value). GPUI CE's
/// debug tree does not report a node's live setting, so the node is found
/// by its role, which nothing else in the launcher window has.
pub fn announcer_of(json: &str) -> (String, String) {
    let tree: serde_json::Value = serde_json::from_str(json).unwrap();
    let nodes = tree["nodes"].as_object().unwrap();
    let labels: Vec<&serde_json::Value> = nodes
        .values()
        .map(|node| &node["aria"])
        .filter(|aria| aria["role"] == "Label")
        .collect();
    assert_eq!(labels.len(), 1, "one announcer: {labels:?}");
    let text = |key: &str| labels[0][key].as_str().unwrap_or_default().to_owned();
    (text("label"), text("value"))
}

/// What the launcher window's announcer says now: its name, checked to be
/// its value too, as AccessKit's macOS adapter announces the value and its
/// Windows and Linux adapters the name.
pub fn announcement(cx: &mut VisualTestContext) -> String {
    let (name, value) = announcer_of(&a11y(cx));
    assert_eq!(name, value, "the announcer's name and value");
    name
}

/// Whether, in the tree `json`, no node is reported as focused through an
/// active descendant and the node GPUI reports as focused is not a row
/// (#132): the focus stays in a field, a menu or a list.
pub fn no_row_has_focus(json: &str) -> bool {
    let tree: serde_json::Value = serde_json::from_str(json).unwrap();
    let nodes = tree["nodes"].as_object().unwrap();
    let row = |id: &str| {
        let role = &nodes[id]["aria"]["role"];
        role == "ListBoxOption" || role == "MenuItem"
    };
    tree["active_descendant_focus"].is_null() && !tree["gpui_focus"].as_str().is_some_and(row)
}
