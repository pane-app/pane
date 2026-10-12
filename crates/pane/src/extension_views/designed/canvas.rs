//! The canvas: a leaf the extension draws into (#242, the custom view's
//! successor). Its drawing operations — paths with fills and strokes,
//! rectangles and circles, text and images, clip and transform — paint
//! through GPUI's [`canvas`] and the path builder, never one element per
//! shape: a run of shape operations between text and image ones is one
//! GPUI canvas, so the order the tree paints them in is kept, and a
//! picker of twenty swatches is one element, not twenty. Text and image
//! operations draw as the positioned elements GPUI's own text and images
//! are, above the shapes they follow.
//!
//! The window machinery the custom view held generalizes onto it: the
//! drawing area's bounds are tracked as it is laid out (pointer positions
//! arrive in the canvas's own coordinates, and the size the layout gave
//! it is named in the view's render context, a change asking the view to
//! draw again), a drag continues outside it, and the window's
//! deactivation ends one whose release the window can no longer see.
//! Input rides the designed tree's events: the pointer's down, move (a
//! drag's, coalesced to the latest while one is in flight, as the custom
//! view's were), up, enter and leave, the wheel, a double click and the
//! secondary button, each carrying the point in the canvas's space and
//! the modifiers held; keys ride the node's own `onKey` as every
//! focusable node's do, all non-reserved keys with their modifiers — Tab,
//! Enter and Escape stay with Pane, as they always have. A canvas naming
//! the semantic handlers (`onIncrement`, `onDecrement`, `onActivate`)
//! takes the up and down arrows and Space itself, so a control-like
//! canvas needs no key parsing.
//!
//! The canvas is one node to assistive technology, with the role its
//! tree named from the widened set, its label and its value; the drawing
//! adds no nodes of its own. It is keyed like every stateful node, so
//! its focus, its drag and its bounds survive a re-render that still
//! draws it.

use std::cell::Cell;
use std::rc::Rc;

use core::fmt::Write as _;

use gpui::canvas as gpui_canvas;
use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, ContentMask, DispatchPhase, FillOptions, FontWeight, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, PathStyle, Pixels, Point, Role,
    ScrollWheelEvent, SharedString, Size, StrokeOptions, WeakEntity, Window, div, px,
};
use lyon::tessellation::{LineCap, LineJoin};

use pane_core::{Canvas as CanvasNode, CanvasHandlers, CanvasOp, DesignedHandler, Finite, Node};

use crate::app::LauncherWindow;
use crate::ui::extension_icon::IconSize;
use crate::ui::tokens;

use super::components::{self, short};
use super::reconcile::Held;
use super::tree::Draw;
use super::{Activate, Decrement, Increment};

/// One layer of a canvas's drawing, in the order the tree paints them: a
/// run of shape operations (painted together, through one GPUI canvas),
/// one line of text, or one image — the latter two drawn as the positioned
/// elements GPUI's own are, above the shapes they follow.
enum Layer {
    /// The shape operations of one run, their colours resolved onto the
    /// theme in force.
    Shapes(Vec<Painted>),
    /// One line of text, drawn as an element.
    Text(Box<Texted>),
    /// One image of the icon model, drawn as an element while it loads as
    /// its fallback or nothing.
    Image {
        image: Option<pane_core::Icon>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}

/// One shape operation as the canvas paints it: the tree's operation with
/// its colours resolved onto the theme in force. Rectangles and circles
/// expand into path operations at resolution time (a rectangle's corners
/// as curves, a circle as four), so the painting walks one grammar.
#[derive(Clone, Copy)]
enum Painted {
    Move {
        x: f32,
        y: f32,
    },
    Line {
        x: f32,
        y: f32,
    },
    Quad {
        cx: f32,
        cy: f32,
        x: f32,
        y: f32,
    },
    Cubic {
        c1x: f32,
        c1y: f32,
        c2x: f32,
        c2y: f32,
        x: f32,
        y: f32,
    },
    Arc {
        x: f32,
        y: f32,
        radius: f32,
        start: f32,
        end: f32,
        ccw: bool,
    },
    Close,
    Fill {
        color: gpui::Hsla,
    },
    Stroke {
        stroke: Stroked,
    },
    Clip {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    Translate {
        x: f32,
        y: f32,
    },
    Scale {
        x: f32,
        y: f32,
    },
    Rotate {
        degrees: f32,
    },
}

/// One stroke as the canvas paints it: its colour resolved (a stroke is a
/// drawing, not text: never corrected), its width in pixels, and the
/// shapes of its ends and corners as the tree named them.
#[derive(Clone, Copy)]
struct Stroked {
    color: gpui::Hsla,
    width: f32,
    cap: Option<pane_core::StrokeCap>,
    join: Option<pane_core::StrokeJoin>,
}

/// One line of text the canvas draws, as the element GPUI's own text is:
/// its position, its content, and how it is drawn — its colour corrected
/// against the canvas's surface as a text's is.
struct Texted {
    x: f32,
    y: f32,
    content: SharedString,
    size: f32,
    weight: f32,
    line_height: f32,
    family: SharedString,
    color: gpui::Hsla,
}

/// The transform the drawing state holds: the composition of the
/// translate, scale and rotate operations before it, applied to every
/// point the operations that follow name. Row-major:
/// `x' = a·x + c·y + e`, `y' = b·x + d·y + f`.
#[derive(Clone, Copy)]
struct Affine {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Affine {
    fn identity() -> Affine {
        Affine {
            a: 1.,
            b: 0.,
            c: 0.,
            d: 1.,
            e: 0.,
            f: 0.,
        }
    }

    /// The composition that applies `then` after this.
    fn then(self, then: Affine) -> Affine {
        Affine {
            a: self.a * then.a + self.c * then.b,
            b: self.b * then.a + self.d * then.b,
            c: self.a * then.c + self.c * then.d,
            d: self.b * then.c + self.d * then.d,
            e: self.a * then.e + self.c * then.f + self.e,
            f: self.b * then.e + self.d * then.f + self.f,
        }
    }

    /// The point `(x, y)` as this transform places it.
    fn of(self, x: f32, y: f32) -> Point<Pixels> {
        gpui::point(
            px(self.a * x + self.c * y + self.e),
            px(self.b * x + self.d * y + self.f),
        )
    }

    /// The vector `(x, y)` as this transform scales and rotates it.
    fn of_vector(self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y, self.b * x + self.d * y)
    }

    fn translate(x: f32, y: f32) -> Affine {
        Affine {
            e: x,
            f: y,
            ..Affine::identity()
        }
    }

    fn scale(x: f32, y: f32) -> Affine {
        Affine {
            a: x,
            d: y,
            ..Affine::identity()
        }
    }

    fn rotate(degrees: f32) -> Affine {
        let (sine, cosine) = degrees.to_radians().sin_cos();
        Affine {
            a: cosine,
            b: sine,
            c: -sine,
            d: cosine,
            ..Affine::identity()
        }
    }
}

/// One canvas node: its drawing painted in one GPUI canvas per run of
/// shapes, its text and image operations as positioned elements, and its
/// input and accessibility on the one node it is. `path` is the node's
/// place in the tree, which keeps its focus, its drag and its bounds
/// across re-renders.
pub(super) fn canvas(
    node: &Node,
    held: &CanvasNode,
    path: &str,
    draw: &Draw,
    cx: &mut gpui::Context<LauncherWindow>,
) -> AnyElement {
    // The drawing, resolved onto the theme in force: shape runs, text and
    // image layers, in the order the tree paints them.
    let layers = resolve(&held.ops, draw);
    // The keyed state the reconciler holds: the focus handle, and where
    // the drawing area was last laid out, which the canvas that paints it
    // compares against. The drag a press holds is read by the events that
    // move it, by the canvas's place in the tree.
    let tracked = draw.state.get(path).and_then(|state| match &state.held {
        Held::Canvas { focus, bounds, .. } => Some((Some(focus.clone()), bounds.clone())),
        _ => None,
    });
    let (focus, bounds) = tracked.unwrap_or_else(|| (None, Rc::new(Cell::new(Bounds::default()))));

    // What the events the canvas raises carry: the node's key, the render
    // whose tree is drawn, and the handlers the tree named, captured as
    // the drawing is built, as a button's press captures its own.
    let key = node.key.clone().unwrap_or_default();
    let render = draw.render;
    let handlers = held.handlers;

    // One node to assistive technology: the role its tree named (a generic
    // one when it named none), its label and its value; the drawing adds
    // no nodes of its own.
    let role = |named: Option<pane_core::CanvasRole>| match named {
        Some(pane_core::CanvasRole::ColorWell) => Role::ColorWell,
        Some(pane_core::CanvasRole::Slider) => Role::Slider,
        Some(pane_core::CanvasRole::Image) => Role::Image,
        Some(pane_core::CanvasRole::Figure) => Role::Figure,
        Some(pane_core::CanvasRole::Group) => Role::Group,
        None | Some(pane_core::CanvasRole::Generic) => Role::GenericContainer,
    };
    let label = held.a11y.label.clone().or_else(|| node.name.clone());
    let value = held.a11y.value.clone().unwrap_or_default();
    let debug = format!(
        "designed-canvas-{}",
        short(if key.is_empty() { "canvas" } else { &key })
    );

    // The drawing area: one GPUI canvas per run of shapes (which also
    // tells the window the size the layout gave the area), the text and
    // image operations as positioned elements between them, clipped to
    // the area and filling it. Only a press over the area is a press over
    // the canvas.
    let entity = cx.entity().downgrade();
    // The listeners name the canvas by its place in the tree, which is
    // unique; the event's `key` is the node's own, as every event's is.
    let on = OnCanvas {
        path: path.to_owned(),
        handlers,
        render,
    };
    let (down_on, secondary_on, wheel_on, hover_on) =
        (on.clone(), on.clone(), on.clone(), on.clone());
    let (down_focus, secondary_focus) = (focus.clone(), focus.clone());
    let drawing = div()
        .id(format!("{path}/area"))
        .relative()
        .size_full()
        .overflow_hidden()
        .children(
            layers
                .into_iter()
                .enumerate()
                .map(|(index, layer)| layer_element(layer, index, path, draw, &bounds, &entity)),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                if let Some(focus) = down_focus.as_ref() {
                    window.focus(focus, cx);
                }
                if let Some(at) = this.designed_canvas_point(&down_on.path, event.position) {
                    this.designed_canvas_down(
                        &down_on,
                        at,
                        event.click_count,
                        &event.modifiers,
                        window,
                        cx,
                    );
                }
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                if let Some(focus) = secondary_focus.as_ref() {
                    window.focus(focus, cx);
                }
                if let Some(at) = this.designed_canvas_point(&secondary_on.path, event.position) {
                    this.designed_canvas_secondary(
                        &secondary_on,
                        at,
                        event.click_count,
                        &event.modifiers,
                        window,
                        cx,
                    );
                }
            }),
        )
        .on_scroll_wheel(
            cx.listener(move |this, event: &ScrollWheelEvent, window, cx| {
                if let Some(at) = this.designed_canvas_point(&wheel_on.path, event.position) {
                    this.designed_canvas_wheel(
                        &wheel_on,
                        at,
                        &event.delta,
                        &event.modifiers,
                        window,
                        cx,
                    );
                }
            }),
        )
        .on_hover(cx.listener(move |this, over: &bool, window, cx| {
            this.designed_canvas_hover(&hover_on, *over, window, cx);
        }));

    // The drag: a press over the canvas held, moves and the release sent
    // while it is, anywhere in the window; a move without the button
    // meaning it went up outside, which the window cannot otherwise see.
    let (move_on, up_on) = (on.clone(), on.clone());
    let (moved, released) = (entity.clone(), entity);
    let element = div()
        .id(path.to_owned())
        .key_context(
            if handlers.on_increment.is_some()
                || handlers.on_decrement.is_some()
                || handlers.on_activate.is_some()
            {
                super::CANVAS_SEMANTICS_CONTEXT
            } else {
                super::CANVAS_CONTEXT
            },
        )
        .debug_selector(move || debug.clone())
        .flex()
        .flex_col()
        .min_w(px(0.))
        .min_h(px(0.))
        .size_full()
        .role(role(held.a11y.role))
        .when_some(label, |element, label| element.aria_label(label))
        .when(!value.is_empty(), |element| element.aria_value(value))
        .when_some(focus, |element, focus| element.track_focus(&focus))
        .on_action::<Increment>(cx.listener({
            let on = on.clone();
            move |this, _, window, cx| {
                this.designed_canvas_semantic(&on, on.handlers.on_increment, window, cx);
            }
        }))
        .on_action::<Decrement>(cx.listener({
            let on = on.clone();
            move |this, _, window, cx| {
                this.designed_canvas_semantic(&on, on.handlers.on_decrement, window, cx);
            }
        }))
        .on_action::<Activate>(cx.listener({
            let on = on.clone();
            move |this, _, window, cx| {
                this.designed_canvas_semantic(&on, on.handlers.on_activate, window, cx);
            }
        }))
        .child(drawing)
        .on_mouse_move_all(move |event: &MouseMoveEvent, phase, _, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            if let Some(moved) = moved.upgrade() {
                moved.update(cx, |this, cx| {
                    this.designed_canvas_moved(
                        &move_on,
                        event.position,
                        event.pressed_button,
                        &event.modifiers,
                        window,
                        cx,
                    );
                });
            }
        })
        .on_mouse_up_all(move |event: &MouseUpEvent, phase, _, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            if let Some(released) = released.upgrade() {
                released.update(cx, |this, cx| {
                    this.designed_canvas_released(&up_on, event.position, window, cx);
                });
            }
        });
    element.into_any_element()
}

/// The element of one layer of the drawing: a run of shapes painted in
/// one GPUI canvas (which also tells the window the size the layout gave
/// the drawing area), one line of text, or one image.
fn layer_element(
    layer: Layer,
    index: usize,
    path: &str,
    draw: &Draw,
    bounds: &Rc<Cell<Bounds<Pixels>>>,
    entity: &WeakEntity<LauncherWindow>,
) -> AnyElement {
    match layer {
        Layer::Shapes(ops) => {
            let bounds = bounds.clone();
            let entity = entity.clone();
            let path = path.to_owned();
            gpui_canvas(
                move |area, _, _| area,
                move |area, _, window, cx| {
                    paint(&ops, area, window);
                    // The size the layout gave the drawing area, told to
                    // the view once the frame is done: the window's state
                    // is not updated while it is being painted, and a
                    // change is what the view asked to be told of.
                    let held = bounds.get();
                    if held.size != area.size || held.origin != area.origin {
                        bounds.set(area);
                        // Told once the frame is done, with the window the
                        // event it asks for is sent through: the window's
                        // state is not updated while it is being painted.
                        let entity = entity.clone();
                        let path = path.clone();
                        window.defer(cx, move |window, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                this.designed_canvas_laid_out(&path, area, window, cx);
                            });
                        });
                    }
                },
            )
            .absolute()
            .size_full()
            .into_any_element()
        }
        Layer::Text(text) => text_element(&text),
        Layer::Image {
            image,
            x,
            y,
            width,
            height,
        } => image_element(
            image,
            gpui::point(px(x), px(y)),
            gpui::size(px(width), px(height)),
            index,
            path,
            draw,
        ),
    }
}

/// One line of text, drawn as the element GPUI's own text is: positioned
/// at its operation's corner, in its style's type, adding no node of its
/// own to the tree the canvas's one node is.
fn text_element(text: &Texted) -> AnyElement {
    div()
        .absolute()
        .left(px(text.x))
        .top(px(text.y))
        .flex_none()
        .whitespace_nowrap()
        .text_size(px(text.size))
        .font_weight(FontWeight::from(text.weight))
        .font_family(text.family.clone())
        .line_height(px(text.line_height))
        .text_color(text.color)
        .child(text.content.clone())
        .into_any_element()
}

/// One image of the icon model, drawn as the tree's images are while it
/// loads (its fallback) or nothing, in the box its operation names.
fn image_element(
    image: Option<pane_core::Icon>,
    at: gpui::Point<Pixels>,
    size: Size<Pixels>,
    index: usize,
    path: &str,
    draw: &Draw,
) -> AnyElement {
    let Some(icon) = image else {
        return div().into_any_element();
    };
    let drawn = components::drawn(&icon, draw);
    let inner = crate::ui::extension_icon::draw(
        &drawn,
        IconSize::small(size.width.max(size.height)),
        format!("{path}/image{index}"),
        "designed",
        draw.theme,
    );
    div()
        .absolute()
        .left(at.x)
        .top(at.y)
        .w(size.width)
        .h(size.height)
        .flex_none()
        .overflow_hidden()
        .child(inner.aria_hidden())
        .into_any_element()
}

/// The canvas's operations as the layers it draws, their colours resolved
/// onto the theme in force: rectangles and circles expand into path
/// operations, so one grammar paints them all.
fn resolve(ops: &[CanvasOp], draw: &Draw) -> Vec<Layer> {
    let theme = draw.theme;
    let mut layers = Vec::new();
    let mut run: Vec<Painted> = Vec::new();
    let color = |paint: &Option<pane_core::Paint>| {
        paint
            .as_ref()
            .map(|paint| tokens::paint_color(paint, theme))
    };
    for op in ops {
        // Cloned: the text and image operations own their content, and the
        // rest are plain numbers.
        let op = op.clone();
        match op {
            CanvasOp::Text(text) => {
                if !run.is_empty() {
                    layers.push(Layer::Shapes(std::mem::take(&mut run)));
                }
                layers.push(Layer::Text(Box::new(Texted {
                    x: text.x,
                    y: text.y,
                    content: text.content.as_str().into(),
                    size: sized(text.size, text.style, theme),
                    weight: weighted(text.weight, text.style, theme),
                    line_height: line_height(text.style, theme),
                    family: family(text.style, theme),
                    color: text
                        .color
                        .as_ref()
                        .map(|paint| tokens::foreground(paint, draw.surface, theme))
                        .unwrap_or_else(|| tokens::text_level(text.level, theme)),
                })));
            }
            CanvasOp::Image {
                image,
                x,
                y,
                width,
                height,
            } => {
                if !run.is_empty() {
                    layers.push(Layer::Shapes(std::mem::take(&mut run)));
                }
                layers.push(Layer::Image {
                    image,
                    x,
                    y,
                    width: width.0,
                    height: height.0,
                });
            }
            CanvasOp::Rect {
                x,
                y,
                width,
                height,
                radius,
                fill,
                stroke,
            } => {
                let shape = rectangle(x, y, width, height, radius.map(|Finite(p)| p));
                run.extend(shape.iter().copied());
                if let Some(color) = color(&fill) {
                    run.push(Painted::Fill { color });
                }
                if let Some(stroke) = stroke {
                    run.extend(shape.iter().copied());
                    run.push(Painted::Stroke {
                        stroke: stroked(&stroke, theme),
                    });
                }
            }
            CanvasOp::Circle {
                x,
                y,
                radius,
                fill,
                stroke,
            } => {
                let shape = circle(x, y, radius.0);
                run.extend(shape.iter().copied());
                if let Some(color) = color(&fill) {
                    run.push(Painted::Fill { color });
                }
                if let Some(stroke) = stroke {
                    run.extend(shape.iter().copied());
                    run.push(Painted::Stroke {
                        stroke: stroked(&stroke, theme),
                    });
                }
            }
            CanvasOp::Move { x, y } => run.push(Painted::Move { x, y }),
            CanvasOp::Line { x, y } => run.push(Painted::Line { x, y }),
            CanvasOp::Quad { cx, cy, x, y } => run.push(Painted::Quad { cx, cy, x, y }),
            CanvasOp::Cubic {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => run.push(Painted::Cubic {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            }),
            CanvasOp::Arc {
                x,
                y,
                radius,
                start,
                end,
                ccw,
            } => run.push(Painted::Arc {
                x,
                y,
                radius: radius.0,
                start,
                end,
                ccw,
            }),
            CanvasOp::Close => run.push(Painted::Close),
            CanvasOp::Fill { color } => run.push(Painted::Fill {
                color: tokens::paint_color(&color, theme),
            }),
            CanvasOp::Stroke { stroke } => run.push(Painted::Stroke {
                stroke: stroked(&stroke, theme),
            }),
            CanvasOp::Clip {
                x,
                y,
                width,
                height,
            } => run.push(Painted::Clip {
                x,
                y,
                width: width.0,
                height: height.0,
            }),
            CanvasOp::Translate { x, y } => run.push(Painted::Translate { x, y }),
            CanvasOp::Scale { x, y } => run.push(Painted::Scale { x, y }),
            CanvasOp::Rotate { degrees } => run.push(Painted::Rotate { degrees }),
        }
    }
    if !run.is_empty() {
        layers.push(Layer::Shapes(run));
    }
    layers
}

/// A rectangle's path, `radius` rounding its corners.
fn rectangle(x: f32, y: f32, width: f32, height: f32, radius: Option<f32>) -> Vec<Painted> {
    let radius = radius
        .unwrap_or(0.)
        .clamp(0., (width / 2.).min(height / 2.));
    if radius <= 0. {
        return vec![
            Painted::Move { x, y },
            Painted::Line { x: x + width, y },
            Painted::Line {
                x: x + width,
                y: y + height,
            },
            Painted::Line { x, y: y + height },
            Painted::Close,
        ];
    }
    let (far, bottom) = (x + width, y + height);
    // Each corner is a quadratic curve of its radius; the sides stay
    // straight between them.
    vec![
        Painted::Move { x: x + radius, y },
        Painted::Line { x: far - radius, y },
        Painted::Quad {
            cx: far,
            cy: y,
            x: far,
            y: y + radius,
        },
        Painted::Line {
            x: far,
            y: bottom - radius,
        },
        Painted::Quad {
            cx: far,
            cy: bottom,
            x: far - radius,
            y: bottom,
        },
        Painted::Line {
            x: x + radius,
            y: bottom,
        },
        Painted::Quad {
            cx: x,
            cy: bottom,
            x,
            y: bottom - radius,
        },
        Painted::Line { x, y: y + radius },
        Painted::Quad {
            cx: x,
            cy: y,
            x: x + radius,
            y,
        },
        Painted::Close,
    ]
}

/// A circle's path as four cubic curves: the standard approximation, each
/// control point `4/3·tan(π/8)` of the radius out.
fn circle(x: f32, y: f32, radius: f32) -> Vec<Painted> {
    let k = radius * 4. / 3. * (core::f32::consts::FRAC_PI_2 / 2.);
    vec![
        Painted::Move { x: x + radius, y },
        Painted::Cubic {
            c1x: x + radius,
            c1y: y + k,
            c2x: x + k,
            c2y: y + radius,
            x,
            y: y + radius,
        },
        Painted::Cubic {
            c1x: x - k,
            c1y: y + radius,
            c2x: x - radius,
            c2y: y + k,
            x: x - radius,
            y,
        },
        Painted::Cubic {
            c1x: x - radius,
            c1y: y - k,
            c2x: x - k,
            c2y: y - radius,
            x,
            y: y - radius,
        },
        Painted::Cubic {
            c1x: x + k,
            c1y: y - radius,
            c2x: x + radius,
            c2y: y - k,
            x: x + radius,
            y,
        },
        Painted::Close,
    ]
}

/// One stroke as it paints: its colour resolved, its width in pixels, one
/// when the tree gives none.
fn stroked(stroke: &pane_core::CanvasStroke, theme: &crate::ui::theme::Theme) -> Stroked {
    Stroked {
        color: tokens::paint_color(&stroke.color, theme),
        width: stroke.width.map(|Finite(width)| width).unwrap_or(1.),
        cap: stroke.cap,
        join: stroke.join,
    }
}

/// A text operation's size: its own, or its token style's.
fn sized(
    size: Option<Finite>,
    style: Option<pane_core::TextStyle>,
    theme: &crate::ui::theme::Theme,
) -> f32 {
    let (style_size, _, _) = tokens::text_style(style, theme);
    size.map(|Finite(pixels)| pixels)
        .unwrap_or(style_size.as_f32())
}

/// A text operation's weight: its own, or its token style's.
fn weighted(
    weight: Option<Finite>,
    style: Option<pane_core::TextStyle>,
    theme: &crate::ui::theme::Theme,
) -> f32 {
    let (_, style_weight, _) = tokens::text_style(style, theme);
    weight.map(|Finite(units)| units).unwrap_or(style_weight.0)
}

/// A text operation's family: its token style's.
fn family(style: Option<pane_core::TextStyle>, theme: &crate::ui::theme::Theme) -> SharedString {
    tokens::text_style(style, theme).2
}

/// A text operation's line height: its size times the theme's rhythm.
fn line_height(style: Option<pane_core::TextStyle>, theme: &crate::ui::theme::Theme) -> f32 {
    let (size, _, _) = tokens::text_style(style, theme);
    size.as_f32() * theme.typography.line_height
}

/// Paints `ops` in `area`, the canvas's drawing area: the path operations
/// build the current path, `Fill` and `Stroke` paint and end it, `Clip`
/// narrows what the operations after it paint, and the transform
/// operations move the space they draw in.
fn paint(ops: &[Painted], area: Bounds<Pixels>, window: &mut Window) {
    let origin = area.origin;
    let mut transform = Affine::identity();
    let mut clip = area;
    let mut built: Vec<Painted> = Vec::new();
    for op in ops {
        match *op {
            Painted::Fill { color } => {
                paint_path(&built, Some(color), None, origin, transform, clip, window);
                built.clear();
            }
            Painted::Stroke { stroke } => {
                paint_path(&built, None, Some(stroke), origin, transform, clip, window);
                built.clear();
            }
            Painted::Clip {
                x,
                y,
                width,
                height,
            } => {
                let (a, b) = (transform.of(x, y), transform.of(x + width, y + height));
                let corners = Bounds::from_corners(a, b);
                clip = clip.intersect(&corners);
                built.clear();
            }
            Painted::Translate { x, y } => {
                transform = Affine::translate(x, y).then(transform);
            }
            Painted::Scale { x, y } => {
                transform = Affine::scale(x, y).then(transform);
            }
            Painted::Rotate { degrees } => {
                transform = Affine::rotate(degrees).then(transform);
            }
            path => built.push(path),
        }
    }
}

/// Paints one built path: filled with `fill` and/or stroked with
/// `stroke`, each point placed by `transform` and offset by `origin`,
/// clipped to `clip`.
fn paint_path(
    points: &[Painted],
    fill: Option<gpui::Hsla>,
    stroke: Option<Stroked>,
    origin: Point<Pixels>,
    transform: Affine,
    clip: Bounds<Pixels>,
    window: &mut Window,
) {
    if points.is_empty() {
        return;
    }
    let at = |x: f32, y: f32| {
        let placed = transform.of(x, y);
        gpui::point(placed.x + origin.x, placed.y + origin.y)
    };
    let paint = |window: &mut Window, fill: Option<gpui::Hsla>, stroke: Option<Stroked>| {
        if let Some(color) = fill
            && let Some(path) = build(points, &at, transform, None)
        {
            window.paint_path(path, color);
        }
        if let Some(stroked) = stroke
            && let Some(path) = build(points, &at, transform, Some(stroked))
        {
            window.paint_path(path, stroked.color);
        }
    };
    let mask = ContentMask { bounds: clip };
    window.with_content_mask(Some(mask), |window| {
        paint(window, fill, stroke);
    });
}

/// The path `points` build, placed by `at`: `stroke`'s options when it is
/// a stroke, a fill otherwise.
fn build(
    points: &[Painted],
    at: &dyn Fn(f32, f32) -> Point<Pixels>,
    transform: Affine,
    stroke: Option<Stroked>,
) -> Option<gpui::Path<Pixels>> {
    let mut builder = PathBuilder::default();
    builder.style = match stroke {
        Some(stroke) => {
            let mut options = StrokeOptions::default().with_line_width(stroke.width);
            if let Some(cap) = stroke.cap {
                options = options.with_line_cap(cap_of(cap));
            }
            if let Some(join) = stroke.join {
                options = options.with_line_join(join_of(join));
            }
            PathStyle::Stroke(options)
        }
        None => PathStyle::Fill(FillOptions::default()),
    };
    for point in points {
        match *point {
            Painted::Move { x, y } => builder.move_to(at(x, y)),
            Painted::Line { x, y } => builder.line_to(at(x, y)),
            Painted::Quad { cx, cy, x, y } => builder.curve_to(at(x, y), at(cx, cy)),
            Painted::Cubic {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => builder.cubic_bezier_to(at(x, y), at(c1x, c1y), at(c2x, c2y)),
            Painted::Arc {
                x,
                y,
                radius,
                start,
                end,
                ccw,
            } => {
                let (sx, sy) = (x + radius * start.cos(), y + radius * start.sin());
                let (ex, ey) = (x + radius * end.cos(), y + radius * end.sin());
                // An arc under the transform is an elliptical one: the
                // radius vector along each axis as the transform places
                // it, its angle the rotation the basis took.
                let (bx, by) = transform.of_vector(radius, 0.);
                let (ux, uy) = transform.of_vector(0., radius);
                let (r1, r2) = (bx.hypot(by), ux.hypot(uy));
                let rotation = by.atan2(bx).to_degrees();
                let sweep = (end - start).rem_euclid(core::f32::consts::TAU);
                let large = sweep > core::f32::consts::PI;
                // A transform that mirrors (a negative determinant) turns
                // a sweep around with it.
                let mirrored = transform.a * transform.d - transform.b * transform.c < 0.;
                let clockwise = !ccw != mirrored;
                builder.move_to(at(sx, sy));
                builder.arc_to(
                    gpui::point(px(r1), px(r2)),
                    px(rotation),
                    large,
                    clockwise,
                    at(ex, ey),
                );
            }
            Painted::Close => builder.close(),
            _ => {}
        }
    }
    builder.build().ok()
}

/// A stroke's cap, as the path builder takes it.
fn cap_of(cap: pane_core::StrokeCap) -> LineCap {
    match cap {
        pane_core::StrokeCap::Butt => LineCap::Butt,
        pane_core::StrokeCap::Round => LineCap::Round,
        pane_core::StrokeCap::Square => LineCap::Square,
    }
}

/// A stroke's join, as the path builder takes it.
fn join_of(join: pane_core::StrokeJoin) -> LineJoin {
    match join {
        pane_core::StrokeJoin::Miter => LineJoin::Miter,
        pane_core::StrokeJoin::Round => LineJoin::Round,
        pane_core::StrokeJoin::Bevel => LineJoin::Bevel,
    }
}

/// The canvas an event is raised on: its path in the keyed state (which
/// names it and the drag it holds), the handlers its tree named, and the
/// render whose tree is drawn — the trio every canvas event's plumbing
/// carries, captured as the drawing is built, as a button's press
/// captures its own.
#[derive(Clone)]
struct OnCanvas {
    /// The canvas's path in the keyed state.
    path: String,
    /// The handlers the tree named. Copy, so the clone is cheap.
    handlers: CanvasHandlers,
    /// The render whose tree is drawn.
    render: u64,
}

impl LauncherWindow {
    /// The canvas at `path`'s drawing was laid out at `area`: the size is
    /// named in the view's render context from now on, and its change is
    /// told to the view when its tree asked to be — the resize event,
    /// which re-renders it as any event does. Deferred until the frame is
    /// done, by the canvas that paints it.
    pub(crate) fn designed_canvas_laid_out(
        &mut self,
        path: &str,
        area: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let (width, height) = (area.size.width.as_f32(), area.size.height.as_f32());
        let Some(entry) = self.canvas_entry(path) else {
            return;
        };
        let key = entry.1.key.clone();
        let seen = entry.1.render;
        let view = entry.0;
        let Some(handler) = self
            .launcher
            .note_designed_canvas(view, &key, width, height)
        else {
            cx.notify();
            return;
        };
        let payload = event(
            "resize",
            &[number("width", width), number("height", height)],
        );
        self.designed_event(
            DesignedHandler::Resize,
            handler,
            key,
            seen,
            payload,
            window,
            cx,
        );
    }

    /// The window was deactivated while a drag over the canvas at `path`
    /// may be held: the release may then never reach the window, so the
    /// drag ends where the pointer was last seen.
    pub(crate) fn designed_canvas_deactivated(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if let Some(on) = self.designed_canvas_of(path) {
            self.designed_canvas_released_at(&on, window, cx);
        }
    }

    /// `position`, a window position, in the canvas at `path`'s own
    /// coordinates: its drawing area's origin, as it was last laid out.
    fn designed_canvas_point(&self, path: &str, position: Point<Pixels>) -> Option<(f32, f32)> {
        let entry = self.designed.as_ref()?.state.get(path)?;
        let Held::Canvas { bounds, .. } = &entry.held else {
            return None;
        };
        let local = position - bounds.get().origin;
        Some((local.x.as_f32().floor(), local.y.as_f32().floor()))
    }

    /// The view and the canvas's keyed state at `path`, when one is open.
    fn canvas_entry(
        &self,
        path: &str,
    ) -> Option<(pane_core::ViewId, &super::reconcile::KeyedState)> {
        let controls = self.designed.as_ref()?;
        Some((controls.view, controls.state.get(path)?))
    }

    /// The canvas at `path`: the handlers its tree names (the tree now on
    /// screen's, whose render is drawn), and its own key.
    fn designed_canvas_of(&self, path: &str) -> Option<OnCanvas> {
        let (_, entry) = self.canvas_entry(path)?;
        let key = entry.key.clone();
        let render = entry.render;
        let tree = match self.launcher.screen() {
            pane_core::Screen::DesignedView(view) => Some(view.tree),
            _ => None,
        };
        Some(OnCanvas {
            path: path.to_owned(),
            handlers: canvas_at(tree.as_ref(), &key)?,
            render,
        })
    }

    /// The primary button was pressed over the canvas at `path` at `at`:
    /// the drag it holds starts, and its handlers are told — the press,
    /// and a double one when it is.
    fn designed_canvas_down(
        &mut self,
        on: &OnCanvas,
        at: (f32, f32),
        clicks: usize,
        modifiers: &gpui::Modifiers,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if let Some(controls) = self.designed.as_mut()
            && let Some(entry) = controls.state.get_mut(&on.path)
            && let Held::Canvas { drag, .. } = &mut entry.held
        {
            let mut drag = drag.borrow_mut();
            drag.pressed = true;
            drag.last = at;
        }
        if let Some(down) = on.handlers.on_pointer_down {
            let payload = pointer("pointer-down", at, "left", clicks, modifiers);
            self.designed_canvas_send(DesignedHandler::Pointer, on, down, payload, window, cx);
        }
        if clicks >= 2 {
            self.designed_canvas_double(on, at, modifiers, window, cx);
        }
    }

    /// The secondary button was pressed over the canvas at `path` at
    /// `at`.
    fn designed_canvas_secondary(
        &mut self,
        on: &OnCanvas,
        at: (f32, f32),
        clicks: usize,
        modifiers: &gpui::Modifiers,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if let Some(secondary) = on.handlers.on_secondary {
            let payload = pointer("secondary", at, "right", clicks, modifiers);
            self.designed_canvas_send(
                DesignedHandler::Secondary,
                on,
                secondary,
                payload,
                window,
                cx,
            );
        }
    }

    /// The canvas at `path` was double-clicked at `at`: its handler is
    /// told, after the second press's own event.
    fn designed_canvas_double(
        &mut self,
        on: &OnCanvas,
        at: (f32, f32),
        modifiers: &gpui::Modifiers,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if let Some(double) = on.handlers.on_double_click {
            let payload = pointer("double-click", at, "left", 2, modifiers);
            self.designed_canvas_send(
                DesignedHandler::DoubleClick,
                on,
                double,
                payload,
                window,
                cx,
            );
        }
    }

    /// The pointer moved over the window: a drag the canvas at `path`
    /// holds moves, anywhere in the window — its move handler told,
    /// coalesced to the latest while one is in flight; a move without the
    /// button while one is held means it went up outside the window,
    /// unseen, so the drag ends as a release would.
    fn designed_canvas_moved(
        &mut self,
        on: &OnCanvas,
        position: Point<Pixels>,
        pressed: Option<MouseButton>,
        modifiers: &gpui::Modifiers,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let at = self.designed_canvas_point(&on.path, position);
        // Whether a drag is held, and whether this move means the button
        // went up outside the window: the release the window cannot see.
        let held = {
            let Some(controls) = self.designed.as_ref() else {
                return;
            };
            let Some(entry) = controls.state.get(&on.path) else {
                return;
            };
            let Held::Canvas { drag, .. } = &entry.held else {
                return;
            };
            // Nothing is held: plain hovering sends nothing (its events
            // are enter and leave).
            let mut drag = drag.borrow_mut();
            if !drag.pressed {
                false
            } else if pressed != Some(MouseButton::Left) {
                drag.pressed = false;
                true
            } else {
                drag.pressed
            }
        };
        if !held {
            return;
        }
        if pressed != Some(MouseButton::Left) {
            // The button went up outside the window: the drag ends where
            // the pointer was last seen.
            self.designed_canvas_released_at(on, window, cx);
            return;
        }
        let Some(at) = at else {
            return;
        };
        self.designed_canvas_move(on, at, modifiers, window, cx);
    }

    /// A drag's move over the canvas at `path` at `at`, coalesced: while
    /// one is in flight, only the latest waits, and it is sent when the
    /// moves in flight are answered.
    fn designed_canvas_move(
        &mut self,
        on: &OnCanvas,
        at: (f32, f32),
        modifiers: &gpui::Modifiers,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let send = self.designed.as_mut().and_then(|controls| {
            let entry = controls.state.get_mut(&on.path)?;
            let Held::Canvas { drag, .. } = &mut entry.held else {
                return None;
            };
            let mut drag = drag.borrow_mut();
            drag.last = at;
            let movement = on.handlers.on_pointer_move?;
            if drag.in_flight > 0 {
                drag.waiting = Some((at, *modifiers));
                return None;
            }
            drag.in_flight += 1;
            Some(movement)
        });
        let Some(movement) = send else {
            return;
        };
        let payload = pointer("pointer-move", at, "left", 0, modifiers);
        let key = self.canvas_key(&on.path);
        let pending = self.launcher.send_designed_seen(
            DesignedHandler::Pointer,
            movement,
            (!key.is_empty()).then_some(key.as_str()),
            Some(on.render),
            payload,
        );
        let path = on.path.clone();
        cx.spawn_in(window, async move |this, cx| {
            pending.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.designed_canvas_move_answered(&path, window, cx);
            });
        })
        .detach();
    }

    /// The drag's move in flight was answered: the waiting one goes out
    /// now, if one waits.
    fn designed_canvas_move_answered(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let waiting = self.designed.as_mut().and_then(|controls| {
            let entry = controls.state.get_mut(path)?;
            let Held::Canvas { drag, .. } = &mut entry.held else {
                return None;
            };
            let mut drag = drag.borrow_mut();
            drag.in_flight = drag.in_flight.saturating_sub(1);
            if drag.in_flight > 0 {
                return None;
            }
            drag.waiting.take()
        });
        self.sync_screen(window, cx);
        cx.notify();
        if let Some((at, modifiers)) = waiting {
            // The handlers of the tree now on screen, as the waiting move
            // is raised on it.
            if let Some(on) = self.designed_canvas_of(path) {
                self.designed_canvas_move(&on, at, &modifiers, window, cx);
            }
        }
    }

    /// The primary button was released: the canvas at `path`'s drag ends
    /// and its handler is told, wherever the release happened.
    fn designed_canvas_released(
        &mut self,
        on: &OnCanvas,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let at = self.designed_canvas_point(&on.path, position);
        let released = self.designed.as_mut().and_then(|controls| {
            let entry = controls.state.get_mut(&on.path)?;
            let Held::Canvas { drag, .. } = &mut entry.held else {
                return None;
            };
            let mut drag = drag.borrow_mut();
            if let Some(at) = at {
                drag.last = at;
            }
            let held = drag.pressed;
            drag.pressed = false;
            Some(held)
        });
        if !released.unwrap_or(false) {
            return;
        }
        if let (Some(at), Some(up)) = (at, on.handlers.on_pointer_up) {
            self.designed_canvas_send(
                DesignedHandler::Pointer,
                on,
                up,
                pointer("pointer-up", at, "left", 0, &gpui::Modifiers::none()),
                window,
                cx,
            );
        } else {
            self.designed_canvas_released_at(on, window, cx);
        }
    }

    /// The canvas at `path`'s drag ended without the window seeing the
    /// release: its handler is told at the point last seen.
    fn designed_canvas_released_at(
        &mut self,
        on: &OnCanvas,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let last = self.designed.as_mut().and_then(|controls| {
            let entry = controls.state.get_mut(&on.path)?;
            let Held::Canvas { drag, .. } = &mut entry.held else {
                return None;
            };
            let mut drag = drag.borrow_mut();
            let last = drag.last;
            drag.pressed = false;
            Some(last)
        });
        let (Some(last), Some(up)) = (last, on.handlers.on_pointer_up) else {
            return;
        };
        self.designed_canvas_send(
            DesignedHandler::Pointer,
            on,
            up,
            pointer("pointer-up", last, "left", 0, &gpui::Modifiers::none()),
            window,
            cx,
        );
    }

    /// The pointer entered or left the canvas at `path`'s hover: its
    /// handler is told. Enter carries no point — the window learns of the
    /// hover before it sees a position — and neither is a drag.
    fn designed_canvas_hover(
        &mut self,
        on: &OnCanvas,
        over: bool,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let handler = if over {
            on.handlers.on_pointer_enter
        } else {
            on.handlers.on_pointer_leave
        };
        let Some(handler) = handler else {
            return;
        };
        let payload = event(
            if over {
                "pointer-enter"
            } else {
                "pointer-leave"
            },
            &[],
        );
        self.designed_canvas_send(DesignedHandler::Pointer, on, handler, payload, window, cx);
    }

    /// The wheel turned over the canvas at `path` at `at`.
    fn designed_canvas_wheel(
        &mut self,
        on: &OnCanvas,
        at: (f32, f32),
        delta: &gpui::ScrollDelta,
        modifiers: &gpui::Modifiers,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(wheel) = on.handlers.on_wheel else {
            return;
        };
        let (unit, (dx, dy)) = match delta {
            gpui::ScrollDelta::Pixels(delta) => ("pixel", (delta.x.as_f32(), delta.y.as_f32())),
            gpui::ScrollDelta::Lines(delta) => ("line", (delta.x, delta.y)),
        };
        let payload = event(
            "wheel",
            &[
                number("x", at.0),
                number("y", at.1),
                number("dx", dx),
                number("dy", dy),
                text("unit", unit),
                boolean("ctrl", modifiers.control),
                boolean("alt", modifiers.alt),
                boolean("shift", modifiers.shift),
            ],
        );
        self.designed_canvas_send(DesignedHandler::Wheel, on, wheel, payload, window, cx);
    }

    /// One of the canvas at `path`'s semantic handlers ran: increment on
    /// the up arrow, decrement on the down one, activate on Space.
    fn designed_canvas_semantic(
        &mut self,
        on: &OnCanvas,
        handler: Option<u32>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(handler) = handler else {
            return;
        };
        self.designed_canvas_send(
            DesignedHandler::Press,
            on,
            handler,
            "{}".to_owned(),
            window,
            cx,
        );
    }

    /// The node's own key of the canvas at `path`, as its events name it
    /// (empty when the tree gave it none).
    fn canvas_key(&self, path: &str) -> String {
        self.designed
            .as_ref()
            .and_then(|controls| controls.state.get(path))
            .map(|entry| entry.key.clone())
            .unwrap_or_default()
    }

    /// Sends one event of the canvas `on` names — the handler id its tree
    /// named, of `handler`'s kind, with `payload` — and redraws when its
    /// answer arrives.
    fn designed_canvas_send(
        &mut self,
        handler: DesignedHandler,
        on: &OnCanvas,
        callback: u32,
        payload: String,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let key = self.canvas_key(&on.path);
        self.designed_event(handler, callback, key, on.render, payload, window, cx);
    }
}

/// The canvas node keyed `key` in the tree on screen, when it is drawn:
/// its handlers, as the waiting move is raised on the tree now on screen.
fn canvas_at(tree: Option<&pane_core::DesignedTree>, key: &str) -> Option<CanvasHandlers> {
    fn held(node: &pane_core::Node, key: &str) -> Option<CanvasHandlers> {
        match &node.kind {
            pane_core::NodeKind::Canvas(canvas) if node.key.as_deref() == Some(key) => {
                Some(canvas.handlers)
            }
            _ => node
                .fallback
                .as_deref()
                .and_then(|fallback| held(fallback, key))
                .or_else(|| node.children.iter().find_map(|child| held(child, key))),
        }
    }
    held(&tree?.root, key)
}

/// One field of a canvas event's payload.
enum Field<'a> {
    Number(&'static str, f32),
    Text(&'static str, &'a str),
    Boolean(&'static str, bool),
}

/// A canvas event's payload: `{"event": <event>, <fields>}`.
fn event(event: &str, fields: &[Field]) -> String {
    let mut payload = format!("{{\"event\":\"{}\"", escaped(event));
    for field in fields {
        match field {
            Field::Number(name, value) => {
                let _ = write!(payload, ",\"{name}\":{}", rounded(*value));
            }
            Field::Text(name, value) => {
                let _ = write!(payload, ",\"{name}\":\"{}\"", escaped(value));
            }
            Field::Boolean(name, value) => {
                let _ = write!(payload, ",\"{name}\":{value}");
            }
        }
    }
    payload.push('}');
    payload
}

/// A pointer event's payload: where it happened, which button, how many
/// clicks, and the modifiers held.
fn pointer(
    kind: &str,
    at: (f32, f32),
    button: &str,
    clicks: usize,
    modifiers: &gpui::Modifiers,
) -> String {
    event(
        kind,
        &[
            Field::Number("x", at.0),
            Field::Number("y", at.1),
            Field::Text("button", button),
            Field::Number("clicks", clicks as f32),
            Field::Boolean("ctrl", modifiers.control),
            Field::Boolean("alt", modifiers.alt),
            Field::Boolean("shift", modifiers.shift),
        ],
    )
}

fn number(name: &'static str, value: f32) -> Field<'static> {
    Field::Number(name, value)
}

fn text<'a>(name: &'static str, value: &'a str) -> Field<'a> {
    Field::Text(name, value)
}

fn boolean(name: &'static str, value: bool) -> Field<'static> {
    Field::Boolean(name, value)
}

/// A number as a payload names it: no trailing `.0`.
fn rounded(value: f32) -> String {
    if value.fract() == 0. && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// `text` as a JSON string's content, escaped.
fn escaped(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                escaped.push_str(&format!("\\u{:04x}", other as u32));
            }
            other => escaped.push(other),
        }
    }
    escaped
}

/// What measures text for a designed view's canvases
/// (`pane:extension/view.measure-text`, #242): the fonts the window drew
/// the view with. One `style` JSON is read as a canvas text operation's
/// styling is — a token style, a size in pixels, a weight from 100 to 900,
/// properties it does not know ignored, as a tree's are — and the text is
/// shaped in the type they resolve to, its width the longest line's and
/// its height the style's line height times its lines. Text is measured
/// with the theme's typography (its sizes are the same in either
/// appearance).
pub(crate) fn measures_of(
    system: std::sync::Arc<gpui::WindowTextSystem>,
) -> pane_core::TextMeasures {
    std::sync::Arc::new(move |text: &str, style: &str| {
        let theme = crate::ui::theme::Theme::dark();
        // The style names what a canvas text operation does — a token
        // style, a size in pixels, a weight — read without a parser, as
        // the payloads are.
        let named = field_str(style, "style").and_then(|token| match token.as_str() {
            "heading" => Some(pane_core::TextStyle::Heading),
            "title" => Some(pane_core::TextStyle::Title),
            "body" => Some(pane_core::TextStyle::Body),
            "caption" => Some(pane_core::TextStyle::Caption),
            "mono" => Some(pane_core::TextStyle::Mono),
            "small-mono" => Some(pane_core::TextStyle::SmallMono),
            _ => None,
        });
        let (style_size, style_weight, family) = tokens::text_style(named, &theme);
        let size = field_str(style, "size")
            .and_then(|size| size.parse().ok())
            .map(|size: f32| px(size.clamp(1., 128.)))
            .unwrap_or(style_size);
        let weight = field_str(style, "weight")
            .and_then(|weight| weight.parse().ok())
            .map(|weight: f32| FontWeight::from(weight.clamp(100., 900.)))
            .unwrap_or(style_weight);
        let run = gpui::TextRun {
            len: text.len(),
            font: gpui::Font {
                family: family.clone(),
                weight,
                ..Default::default()
            },
            color: gpui::Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
            letter_spacing: None,
        };
        let line_height = size * theme.typography.line_height;
        match system.shape_text(text, size, &[run], None, None) {
            Ok(lines) => {
                let mut extent = gpui::Size::<Pixels>::default();
                for line in &lines {
                    let line = line.size(line_height);
                    extent.width = extent.width.max(line.width);
                    extent.height += line.height;
                }
                (extent.width.as_f32(), extent.height.as_f32())
            }
            Err(_) => (0., 0.),
        }
    })
}

/// The string a JSON object's `"name"` field holds, when it names one.
fn field_str(json: &str, name: &str) -> Option<String> {
    let at = json.find(&format!("\"{name}\""))?;
    let rest = json[at + name.len() + 2..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}
