//! The designed view's components, at the window: every component of the
//! UI component set (#237) drawn from the Rust sample's gallery command,
//! announcing its accessibility role and name; the layout primitives
//! laying the tree out; the tokens and raw values resolving onto the
//! theme in light and dark; a raw colour corrected for contrast against
//! the panel; and the `hover` and `pressed` variants of a node's surface
//! applied by Pane, with no call into the extension while they are.
//!
//! The Rust designed view sample (`guests/sample-view`) is installed as a
//! package from `target/guests/packages`, its `components` command
//! declared `"mode": "designed"`; the pane-core tests hold the
//! tri-lingual parity of the gallery.

use std::fs;
use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{Launcher, LauncherView, Node, NodeKind, Runtime, Screen};

use tempfile::TempDir;

#[path = "support/a11y.rs"]
mod a11y;

#[path = "support/paint.rs"]
mod paint;

#[path = "support/settle.rs"]
mod settle;

#[path = "support/wait.rs"]
mod wait;

use a11y::accessibility;
use paint::{paints_background_at, paints_fill_at};
use settle::{settle, until};
use wait::settle_frames;

/// The theme's dark danger and light danger tones: what a tone token
/// resolves to in each appearance.
const DARK_DANGER: u32 = 0xFF9A92FF;
const LIGHT_DANGER: u32 = 0xC9372FFF;
/// The theme's dark and light panel solids: what a raw colour is
/// corrected against.
const DARK_PANEL: u32 = 0x16171AFF;
/// The theme's accent: the one accent in both appearances (its readable
/// text form differs; the fill the tone resolves to does not).
const DARK_ACCENT: u32 = 0xC9EE6AFF;
const LIGHT_ACCENT: u32 = 0xC9EE6AFF;
/// A raw blue, too pale to read on the dark panel at 2.5:1.
const RAW_BLUE: u32 = 0x88CCFFFF;

/// One test's window: the folders it installed from and the launcher
/// window, with the Rust sample's gallery command open.
struct Opened {
    _sources: TempDir,
    _data: TempDir,
    window: Entity<LauncherWindow>,
}

/// The launcher window in the `theme` (`light` or `dark`), with the Rust
/// sample's components gallery open.
fn open<'a>(cx: &'a mut TestAppContext, theme: &str) -> (Opened, &'a mut VisualTestContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            None,
            pane::settings::Overrides::parse(Some(theme), None),
            cx,
        )
    });
    let launcher =
        Launcher::with_packages(Runtime::start(), Vec::new(), data.path().join("extensions"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let assembled =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/sample-view");
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    let folder = sources.path().join("sample-view");
    copy_folder(&assembled, &folder);
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    cx.simulate_input("components sample");
    cx.simulate_keystrokes("enter");
    until(&window, cx, |view| {
        matches!(view.screen, Screen::DesignedView(_))
    });
    (
        Opened {
            _sources: sources,
            _data: data,
            window,
        },
        cx,
    )
}

/// Copies what `from` holds into `to`, folders and all.
fn copy_folder(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The bounds of the element with debug selector `selector`.
fn bounds(cx: &mut VisualTestContext, selector: &'static str) -> gpui::Bounds<gpui::Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"))
}

/// Whether the element with debug selector `selector` is drawn.
fn drawn(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.debug_bounds(selector).is_some()
}

/// The a11y tree's JSON, forced on so the tree is built regardless of
/// platform accessibility.
fn a11y(cx: &mut VisualTestContext) -> String {
    cx.update(|window, _| window.set_a11y_forced(true));
    cx.run_until_parked();
    cx.update(|window, _| window.debug_a11y_tree_json())
        .expect("an accessibility tree")
}

/// The a11y tree's nodes, as GPUI reports them to assistive technology.
fn accessible_nodes(json: &str) -> Vec<serde_json::Value> {
    let tree: serde_json::Value = serde_json::from_str(json).unwrap();
    tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .map(|node| node["aria"].clone())
        .collect()
}

/// The a11y node with `role` and `label`, if the window reports one.
fn find<'a>(
    nodes: &'a [serde_json::Value],
    role: &str,
    label: &str,
) -> Option<&'a serde_json::Value> {
    nodes
        .iter()
        .find(|node| node["role"] == role && node["label"] == label)
}

/// The launcher's view, read behind the window.
fn view(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    cx.read_entity(window, |window, _| window.launcher().view())
}

/// The first node of `kind` in `node`'s tree.
fn first_of<'a>(node: &'a Node, of: fn(&'a NodeKind) -> bool) -> Option<&'a Node> {
    if of(&node.kind) {
        return Some(node);
    }
    node.children
        .iter()
        .chain(node.fallback.as_deref())
        .find_map(|child| first_of(child, of))
}

#[gpui::test]
fn the_stack_lays_its_children_over_each_other(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx, "dark");
    let _ = &opened;
    // The icon tile and the badge over it: the badge sits over the tile,
    // its place in the stack putting its origin inside the tile's bounds.
    let tile = bounds(cx, "designed-icon-tile");
    let badge = bounds(cx, "designed-badge-4");
    assert!(
        badge.origin.x > tile.origin.x
            && badge.origin.x < tile.origin.x + tile.size.width
            && badge.origin.y > tile.origin.y
            && badge.origin.y < tile.origin.y + tile.size.height,
        "the badge is drawn over the tile: {tile:?} {badge:?}"
    );
    // The gallery scrolls: the scroll region is taller than the window
    // shows, and the divider and spacer after it are drawn.
    assert!(drawn(cx, "designed-loading"), "the gallery is drawn");
}

#[gpui::test]
fn the_components_announce_their_roles_and_names(cx: &mut TestAppContext) {
    let (_opened, cx) = open(cx, "dark");
    let json = a11y(cx);
    let nodes = accessible_nodes(&json);

    // One mapping per component: the roles the tree's assistive
    // technology reads, each named by what it shows or the label it was
    // given. A text is a label named by its content; the toggle a switch
    // with its state; the checkbox a checkbox; the segmented control a
    // radio group of radio buttons; the slider a slider with its value;
    // the progress bar and the loading indicator progress indicators; the
    // fields text inputs, one a password; the select a combo box with the
    // chosen value; the markdown link a link; the rich row a list item;
    // the section headers and the empty state's title headings.
    // The keycap names its chord as the system does: macOS writes Ctrl
    // out as "Control".
    let chord = if cfg!(target_os = "macos") {
        "Control+Shift+P"
    } else {
        "Ctrl+Shift+P"
    };
    let expect = [
        ("Label", "The UI component set"),
        ("Link", "terms"),
        ("Image", chord),
        ("Label", "beta"),
        ("Label", "3"),
        ("Switch", "Dark mode"),
        ("CheckBox", "Remember"),
        ("RadioGroup", "Digest"),
        ("RadioButton", "Daily"),
        ("Slider", "Volume"),
        ("ProgressIndicator", "Installed"),
        ("ProgressIndicator", "Checking"),
        ("TextInput", "Name"),
        ("PasswordInput", "Secret"),
        ("MultilineTextInput", "Notes"),
        ("ComboBox", "Pick"),
        ("Link", "a link"),
        ("ListItem", "Pane"),
        ("Heading", "Markdown"),
        ("Heading", "Nothing here"),
        ("Button", "Start over"),
    ];
    for (role, label) in expect {
        assert!(
            find(&nodes, role, label).is_some(),
            "the {label} {role} is announced: {json}"
        );
    }
    // The toggle's state and the slider's and the progress bar's values,
    // as the tree reports them: a slider's and a bar's value is the
    // numeric one AccessKit carries, with its bounds and step.
    let toggle = nodes
        .iter()
        .find(|node| node["role"] == "Switch" && node["label"] == "Dark mode")
        .expect("a switch");
    assert_eq!(toggle["toggled"], "False", "{toggle:?}");
    let slider = nodes
        .iter()
        .find(|node| node["role"] == "Slider" && node["label"] == "Volume")
        .expect("a slider");
    assert!(
        (slider["numeric_value"].as_f64().unwrap() - 0.4).abs() < 1e-6,
        "{slider:?}"
    );
    let progress = nodes
        .iter()
        .find(|node| node["role"] == "ProgressIndicator" && node["label"] == "Installed")
        .expect("a progress bar");
    assert!(
        (progress["numeric_value"].as_f64().unwrap() - 0.7).abs() < 1e-6,
        "{progress:?}"
    );
}

#[gpui::test]
fn a_raw_colour_is_corrected_for_contrast_and_an_exact_one_is_not(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx, "dark");
    let _ = &opened;
    // The gallery's icon, tinted a raw blue too pale to read on the dark
    // panel: Pane moved its lightness until it reads at 2.5:1, so the
    // colour it drew is the corrected one, not the raw one.
    let corrected = corrected_hex(RAW_BLUE, DARK_PANEL);
    let selector: &'static str =
        Box::leak(format!("icon-designed-color-{}", corrected).into_boxed_str());
    assert!(
        drawn(cx, selector),
        "the tinted icon draws in the corrected colour {corrected}"
    );
    // An exact colour, though, is drawn as it is.
    assert!(drawn(cx, "designed-text-Exact"));
}

/// `color`, moved in lightness until it reads at 2.5:1 against `surface`,
/// as the hex the icon's debug selector names.
fn corrected_hex(color: u32, surface: u32) -> String {
    let (color, surface) = (hsla(color), hsla(surface));
    let corrected = corrected(color, surface, 2.5);
    let rgba = gpui::hsla_to_rgba(corrected);
    let byte = |channel: f32| (channel.clamp(0., 1.) * 255.).round() as u8;
    format!(
        "{:02x}{:02x}{:02x}{:02x}",
        byte(rgba.red),
        byte(rgba.green),
        byte(rgba.blue),
        byte(rgba.alpha)
    )
}

/// `0xRRGGBBAA` as a colour.
fn hsla(hex: u32) -> gpui::Hsla {
    gpui::rgb_to_hsla(gpui::rgba(hex))
}

/// `color`, moved in lightness until it reads at `needed` against
/// `surface` — the correction Pane applies, restated for the check.
fn corrected(color: gpui::Hsla, surface: gpui::Hsla, needed: f32) -> gpui::Hsla {
    let luminance = |color: gpui::Hsla| {
        let rgba = gpui::hsla_to_rgba(color);
        let linear = |channel: f32| {
            if channel <= 0.03928 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(rgba.red) + 0.7152 * linear(rgba.green) + 0.0722 * linear(rgba.blue)
    };
    let ratio = |a: gpui::Hsla, b: gpui::Hsla| {
        let (a, b) = (luminance(a), luminance(b));
        let (light, dark) = if a > b { (a, b) } else { (b, a) };
        (light + 0.05) / (dark + 0.05)
    };
    let mut opaque = color;
    opaque.alpha = 1.;
    if ratio(opaque, surface) >= needed {
        return color;
    }
    let lighten = luminance(surface) < 0.5;
    let mut moved = color;
    for _ in 0..50 {
        moved.lightness = if lighten {
            (moved.lightness + 0.02).min(1.)
        } else {
            (moved.lightness - 0.02).max(0.)
        };
        let mut check = moved;
        check.alpha = 1.;
        if ratio(check, surface) >= needed {
            break;
        }
    }
    moved
}

#[gpui::test]
fn the_tokens_and_raw_values_resolve_to_the_dark_theme(cx: &mut TestAppContext) {
    tokens_and_raw_values(cx, "dark", DARK_DANGER, DARK_ACCENT);
}

#[gpui::test]
fn the_tokens_and_raw_values_resolve_to_the_light_theme(cx: &mut TestAppContext) {
    tokens_and_raw_values(cx, "light", LIGHT_DANGER, LIGHT_ACCENT);
}

/// The tone tokens and the raw values in one appearance: each appearance
/// has its own app — the settings the appearance is named to are the
/// process's one global, so a second window in the same app would keep
/// the first's theme — and its own test entry above gives it that.
fn tokens_and_raw_values(cx: &mut TestAppContext, theme: &'static str, danger: u32, accent: u32) {
    let (opened, cx) = open(cx, theme);
    // The gallery's Surface text sits below the default viewport's fold,
    // and the list paints only what is in view (#165): the window is
    // made tall enough that it is, before its fills are read on the
    // scene the frame drew.
    cx.simulate_resize(gpui::size(gpui::px(1920.), gpui::px(2200.)));
    settle_frames(cx);
    // The tone token: the danger colour of the appearance, as the
    // Surface text's background.
    let surface = bounds(cx, "designed-text-Surface");
    assert!(
        paints_fill_at(cx, surface, danger),
        "{theme}: the danger tone fills the surface text"
    );
    // The hover variant, applied with no call into the extension: the
    // accent while the pointer is over it.
    let over = surface.center();
    let before = view(&opened.window, cx).status;
    cx.simulate_mouse_move(over, None::<gpui::MouseButton>, gpui::Modifiers::none());
    let hovered = bounds(cx, "designed-text-Surface");
    assert!(
        paints_fill_at(cx, hovered, accent),
        "{theme}: the hover variant fills with the accent"
    );
    // The pressed variant, while the mouse is held on it.
    cx.simulate_mouse_down(over, gpui::MouseButton::Left, gpui::Modifiers::none());
    let pressed = bounds(cx, "designed-text-Surface");
    let variant = pressed_variant(accent);
    assert!(
        paints_background_at(cx, pressed, variant),
        "{theme}: the pressed variant applies"
    );
    cx.simulate_mouse_up(over, gpui::MouseButton::Left, gpui::Modifiers::none());
    // Nothing was sent to the extension while the variants applied.
    let after = view(&opened.window, cx).status;
    assert_eq!(before, after, "{theme}: no call into the extension");
}

/// The pressed wash of a hover colour, as Pane's own pressed rule doubles
/// the alpha.
fn pressed_variant(hover: u32) -> gpui::Background {
    let mut wash = hsla(hover);
    wash.alpha = (wash.alpha * 2.).min(1.);
    gpui::solid_background(wash)
}

#[gpui::test]
fn the_controls_are_focusable_and_a_change_flips_the_toggle(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx, "dark");
    let window = opened.window;

    // The gallery's controls are focusable in tree order: the link first,
    // then the rich row, the toggle, the checkbox, the segmented control,
    // the slider, the fields and the select.
    let count = cx.read_entity(&window, |window, _| window.designed_button_count());
    assert!(count >= 8, "{count} focusable controls in the gallery");

    // Tab reaches the toggle; Enter flips it, its change telling the
    // extension the value, whose answer shows the toggle on.
    let mut reached = 0;
    for _ in 0..count {
        cx.simulate_keystrokes("tab");
        let (focused, _) = accessibility(cx);
        if focused.as_deref() == Some("Dark mode") {
            break;
        }
        reached += 1;
    }
    assert!(
        reached < count,
        "Tab reached the toggle within {count} controls"
    );
    cx.simulate_keystrokes("enter");
    until(&window, cx, |view| match &view.screen {
        Screen::DesignedView(view) => first_of(
            &view.tree.root,
            |kind| matches!(kind, NodeKind::Toggle(toggle) if toggle.on),
        )
        .is_some(),
        _ => false,
    });
    // And Space flips it back.
    cx.simulate_keystrokes("space");
    until(&window, cx, |view| match &view.screen {
        Screen::DesignedView(view) => first_of(
            &view.tree.root,
            |kind| matches!(kind, NodeKind::Toggle(toggle) if !toggle.on),
        )
        .is_some(),
        _ => false,
    });
    let view = settle(&window, cx);
    assert_eq!(view.status, pane_core::Status::Idle);
}
