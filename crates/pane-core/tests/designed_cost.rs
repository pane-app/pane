//! The cost of reading and reconciling a designed view's tree (#237,
//! acceptance item, not a gate): a 500-node and a 5,000-node document,
//! read by the parser and walked the way the window reconciles a new
//! tree (the paths of its focusable controls, by key and place), with
//! the timings printed for the verify run's log to record. Nothing is
//! asserted: the numbers are the deliverable.

use std::time::Instant;

use pane_core::{DesignedTree, Node, NodeKind};

#[test]
fn the_cost_of_reading_and_reconciling_trees() {
    for nodes in [500, 5_000] {
        let document = document_of(nodes);
        let started = Instant::now();
        let tree = DesignedTree::read(&document).expect("the tree reads");
        let read = started.elapsed();
        let started = Instant::now();
        let mut controls = 0;
        let mut path = String::new();
        walk(&tree.root, &mut path, &mut controls);
        let walked = started.elapsed();
        println!(
            "a {}-node tree ({controls} focusable): {read:?} to parse, {walked:?} to reconcile",
            nodes,
        );
    }
}

/// A document of about `nodes` nodes: a column of rows, each a text and a
/// button, the buttons named by keys.
fn document_of(nodes: usize) -> String {
    let rows = nodes / 3;
    let mut document =
        String::from(r#"{"version":"1.1","root":{"type":"column","gap":"m","children":["#);
    for row in 0..rows {
        if row > 0 {
            document.push(',');
        }
        let text = format!(r#"{{"type":"text","text":"Row {row}","level":"secondary"}},"#);
        let button = format!(
            r#"{{"type":"button","key":"row{row}","label":"Press {row}","onPress":{}}}"#,
            row % 32 + 1
        );
        document.push_str(&text);
        document.push_str(&button);
    }
    document.push_str("]}}");
    document
}

/// The walk the window does over a new tree: each node's place composed
/// into a path, the focusable controls collected.
fn walk(node: &Node, path: &mut String, controls: &mut usize) {
    let start = path.len();
    if is_focusable(&node.kind) {
        *controls += 1;
    }
    for (index, child) in node.children.iter().enumerate() {
        path.push('/');
        match child.key.as_deref() {
            Some(key) => path.push_str(key),
            None => path.push_str(&index.to_string()),
        }
        walk(child, path, controls);
        path.truncate(start);
    }
}

/// Whether a node of this kind is a focusable control, as the window
/// decides.
fn is_focusable(kind: &NodeKind) -> bool {
    match kind {
        NodeKind::Button(button) => button.on_press.is_some() && button.enabled,
        NodeKind::Link(link) => link.on_press.is_some(),
        NodeKind::RichRow(row) => row.on_press.is_some(),
        NodeKind::Toggle(toggle) => toggle.on_change.is_some(),
        NodeKind::Checkbox(checkbox) => checkbox.on_change.is_some(),
        NodeKind::Segmented(segmented) => segmented.on_change.is_some(),
        NodeKind::Slider(slider) => slider.on_change.is_some(),
        NodeKind::TextInput(_)
        | NodeKind::PasswordInput(_)
        | NodeKind::TextArea(_)
        | NodeKind::Select(_) => true,
        _ => false,
    }
}
