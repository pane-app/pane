//! The custom view screen: a view an extension draws and whose input it
//! handles.
//!
//! Pane paints the view's latest frame (rectangles and text, in order) in a
//! fixed-size area, and keeps focus, the focus ring and the accessibility
//! node itself: one focusable node with the view's role, label and value.
//! While the view has focus, arrow keys, Home and End go to the extension;
//! Tab, Enter and Escape stay with the launcher. Pressing the primary button
//! over the view, moving while it is held (anywhere in the window) and
//! releasing it go to the extension as pointer events, in view coordinates.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, FocusHandle, KeyBinding, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Role, Subscription, Window, actions,
    canvas, div, prelude::*, px, rgb,
};
use pane_core::{CustomViewRole, CustomViewSnapshot, Key, Point, Shape, ViewEvent, ViewId};

use crate::LauncherWindow;

actions!(custom_view, [Left, Right, Up, Down, Home, End]);

const CONTEXT: &str = "CustomView";

/// Registers the keys a focused custom view receives.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("left", Left, Some(CONTEXT)),
        KeyBinding::new("right", Right, Some(CONTEXT)),
        KeyBinding::new("up", Up, Some(CONTEXT)),
        KeyBinding::new("down", Down, Some(CONTEXT)),
        KeyBinding::new("home", Home, Some(CONTEXT)),
        KeyBinding::new("end", End, Some(CONTEXT)),
    ]);
}

/// What the window keeps for the open custom view.
pub(crate) struct CustomViewControls {
    /// The opened view these controls are for.
    view: ViewId,
    focus: FocusHandle,
    /// Where the view's drawing area was last laid out, to turn window
    /// positions into view coordinates.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// Where the pointer was last seen during a press, in view coordinates:
    /// where a drag ends when the window stops seeing the button.
    last_point: Cell<Point>,
    /// Ends a drag when the window is deactivated, since the release may
    /// then never reach it.
    _deactivation: Subscription,
}

impl LauncherWindow {
    /// Creates or drops the custom view's controls to match the launcher's
    /// screen: a newly opened view, even one replacing another at once,
    /// takes focus, and focus returns to the list when the view closes.
    pub(crate) fn sync_custom_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = self.launcher.view().custom_view().map(|view| view.id);
        let shown = self.custom_view.as_ref().map(|controls| controls.view);
        if open == shown {
            return;
        }
        self.custom_view = open.map(|view| {
            let focus = cx.focus_handle().tab_stop(true);
            window.focus(&focus, cx);
            let deactivation = cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.end_drag(window, cx);
                }
            });
            CustomViewControls {
                view,
                focus,
                bounds: Rc::default(),
                last_point: Cell::new(Point { x: 0, y: 0 }),
                _deactivation: deactivation,
            }
        });
        if open.is_none() {
            window.focus(&self.focus_handle, cx);
        }
    }

    /// Sends `event` to the view and redraws when its answer arrives.
    fn send_view_event(&mut self, event: ViewEvent, window: &mut Window, cx: &mut Context<Self>) {
        let pending = self.launcher.send_view_event(event);
        self.show_until_done(pending, window, cx);
    }

    /// Sends a pointer move or release of a press held over the view, at
    /// window position `position`; nothing without one.
    fn send_drag_event(
        &mut self,
        event: fn(Point) -> ViewEvent,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.launcher.pointer_held() {
            return;
        }
        if let Some(at) = self.view_point(position) {
            if let Some(controls) = &self.custom_view {
                controls.last_point.set(at);
            }
            self.send_view_event(event(at), window, cx);
        }
    }

    /// Ends a held press where the pointer was last seen, when the window
    /// can no longer see the button's release.
    fn end_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.custom_view.as_ref().map(|c| c.last_point.get()) else {
            return;
        };
        if self.launcher.pointer_held() {
            self.send_view_event(ViewEvent::PointerUp(at), window, cx);
        }
    }

    /// `position`, a window position, in the view's coordinates.
    fn view_point(&self, position: gpui::Point<Pixels>) -> Option<Point> {
        let origin = self.custom_view.as_ref()?.bounds.get().origin;
        let local = position - origin;
        Some(Point {
            x: local.x.as_f32().floor() as i32,
            y: local.y.as_f32().floor() as i32,
        })
    }

    /// The custom view screen for the launcher's snapshot of the view.
    pub(crate) fn render_custom_view(
        &self,
        view: CustomViewSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(controls) = &self.custom_view else {
            return div().into_any_element();
        };
        let frame = view.frame;
        let bounds = controls.bounds.clone();
        let focus = controls.focus.clone();
        let entity = cx.entity().downgrade();
        let (moved, released) = (entity.clone(), entity);
        let drawing = div()
            .debug_selector(|| "custom-view".into())
            .relative()
            .w(px(frame.width as f32))
            .h(px(frame.height as f32))
            .overflow_hidden()
            // Only a press over the drawing area is a press over the view;
            // one on the focus ring's padding or border only focuses it.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    if let Some(at) = this.view_point(event.position) {
                        if let Some(controls) = &this.custom_view {
                            controls.last_point.set(at);
                        }
                        this.send_view_event(ViewEvent::PointerDown(at), window, cx);
                    }
                }),
            )
            .children(frame.shapes.into_iter().map(shape))
            .child(
                // Records where the drawing area is, for pointer positions.
                canvas(move |area, _, _| bounds.set(area), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            );
        let view = div()
            .id("custom-view")
            .key_context(CONTEXT)
            .track_focus(&controls.focus)
            .role(match view.role {
                CustomViewRole::ColorWell => Role::ColorWell,
            })
            .aria_label(view.label)
            .aria_value(frame.value)
            .on_action(key_listener::<Left>(cx, Key::Left))
            .on_action(key_listener::<Right>(cx, Key::Right))
            .on_action(key_listener::<Up>(cx, Key::Up))
            .on_action(key_listener::<Down>(cx, Key::Down))
            .on_action(key_listener::<Home>(cx, Key::Home))
            .on_action(key_listener::<End>(cx, Key::End))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&focus, cx);
            })
            // A drag continues outside the view: moves and the release are
            // sent while a press over the view is held. A move without the
            // button means it was released outside the window, unseen.
            .on_mouse_move_all(move |event: &MouseMoveEvent, phase, _, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let held = event.pressed_button == Some(MouseButton::Left);
                let event_for = if held {
                    ViewEvent::PointerMove
                } else {
                    ViewEvent::PointerUp
                };
                moved
                    .update(cx, |this, cx| {
                        this.send_drag_event(event_for, event.position, window, cx)
                    })
                    .ok();
            })
            .on_mouse_up_all(move |event: &MouseUpEvent, phase, _, window, cx| {
                if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                    released
                        .update(cx, |this, cx| {
                            this.send_drag_event(ViewEvent::PointerUp, event.position, window, cx)
                        })
                        .ok();
                }
            })
            .p_1()
            .rounded_md()
            .border_2()
            .border_color(rgb(0x20252d))
            .focus(|node| node.border_color(rgb(0x8ab4f8)))
            .child(drawing);
        // Fills the body, like the list and the form, so the status line
        // stays at the bottom.
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_start()
            .child(view)
            .into_any_element()
    }
}

/// A listener for the key action `A` that sends `key` to the view.
fn key_listener<A: gpui::Action>(
    cx: &mut Context<LauncherWindow>,
    key: Key,
) -> impl Fn(&A, &mut Window, &mut App) + 'static {
    cx.listener(move |this, _: &A, window, cx| {
        this.send_view_event(ViewEvent::Key(key), window, cx)
    })
}

/// A shape of the view's frame, positioned in the drawing area.
fn shape(shape: Shape) -> AnyElement {
    match shape {
        Shape::Rect {
            x,
            y,
            width,
            height,
            fill,
        } => div()
            .absolute()
            .left(px(x as f32))
            .top(px(y as f32))
            .w(px(width as f32))
            .h(px(height as f32))
            .bg(rgb(fill.0))
            .into_any_element(),
        Shape::Text {
            x,
            y,
            content,
            color,
        } => div()
            .absolute()
            .left(px(x as f32))
            .top(px(y as f32))
            .whitespace_nowrap()
            .text_color(rgb(color.0))
            .child(content)
            .into_any_element(),
    }
}
