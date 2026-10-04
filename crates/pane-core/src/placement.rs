//! Launcher placement: which display the launcher window opens on.
//!
//! The *choice* is a host setting the Launcher page records
//! ([`crate::host_settings::OpeningMonitor`]); this module is the
//! behavior half that is the same everywhere — the display layout as the
//! platform reports it, and the resolution that turns the choice into the
//! display the launcher opens on, with a fallback when the chosen display
//! is gone. Reading the layout and moving a window are the renderer
//! layer's own work (see the `pane` crate's `placement` module, which
//! reaches the displays through GPUI CE and the two things it cannot do —
//! naming the display of the window the user is working in, and moving a
//! window that already exists — through Pane's own platform code); the
//! model here is deliberately renderer-independent, in plain numbers, so
//! the resolution's every rule is testable without a window.
//!
//! The three truths the placement keeps apart, as the General page does
//! for the Open Pane hotkey and the login registration: the user's
//! *choice* (the record's field), what the platform can *tell* (the
//! layout's `pointer` and `active` are `None` where the system does not
//! answer, and the page explains the choice rather than offering it), and
//! what opening *does* (the resolution, which falls back to the primary
//! display — or the first available one — when the chosen display is
//! disconnected or unknown, keeping the launcher's controls reachable
//! rather than placing its window off-screen).

use crate::host_settings::OpeningMonitor;

/// A point in the placement's own units: logical pixels of the display
/// layout, the same space the displays' bounds are in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// A size in the placement's own units; see [`Point`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

/// A rectangle in the placement's own units; see [`Point`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

impl Rect {
    /// Whether `point` lies inside this rectangle. A point on the edge
    /// counts as inside, as a pointer held at the very edge of a display
    /// still is on that display.
    pub fn contains(&self, point: Point) -> bool {
        (self.origin.x..=self.origin.x + self.size.width).contains(&point.x)
            && (self.origin.y..=self.origin.y + self.size.height).contains(&point.y)
    }

    /// The center of this rectangle.
    pub fn center(&self) -> Point {
        Point {
            x: self.origin.x + self.size.width / 2.,
            y: self.origin.y + self.size.height / 2.,
        }
    }
}

/// The platform's own identity of a display, as the layout reports it.
/// Pane does not read these numbers' meaning: they only have to match the
/// ones the layout lists, so a choice's display is found by the identity
/// it was named with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DisplayId(pub u64);

/// One display, as the layout reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    /// The display's identity, as the platform names it.
    pub id: DisplayId,
    /// The display's whole bounds, in the layout's units.
    pub bounds: Rect,
    /// The display's usable area — the part a window's controls stay
    /// reachable in, clear of the taskbar or dock. The launcher is placed
    /// within this, not merely within the display.
    pub usable: Rect,
}

impl Display {
    /// The bounds a window of `size` opens at on this display: centered
    /// in the usable area, and clamped to it, so the window never starts
    /// with part of itself — its controls included — off the display. A
    /// window larger than the usable area itself keeps its size and sits
    /// at the area's top left, the most of it the display can show.
    pub fn window_bounds(&self, size: Size) -> Rect {
        let usable = self.usable;
        let origin = Point {
            x: usable.center().x - size.width / 2.,
            y: usable.center().y - size.height / 2.,
        };
        let fits = size.width <= usable.size.width && size.height <= usable.size.height;
        Rect {
            origin: if fits {
                Point {
                    x: origin.x.clamp(
                        usable.origin.x,
                        usable.origin.x + usable.size.width - size.width,
                    ),
                    y: origin.y.clamp(
                        usable.origin.y,
                        usable.origin.y + usable.size.height - size.height,
                    ),
                }
            } else {
                usable.origin
            },
            size,
        }
    }
}

/// The displays as they are now: every one, the primary, and where the
/// platform can tell them, the pointer's position and the display of the
/// window the user is working in. `None` is the honest answer of a
/// platform that cannot tell — the Launcher page explains the choice
/// instead of offering it, and the resolution falls back to the primary
/// display — never a guess.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayLayout {
    /// Every connected display.
    pub displays: Vec<Display>,
    /// The primary display's identity, as the layout's own `displays`
    /// name it; `None` when the platform names no primary.
    pub primary: Option<DisplayId>,
    /// The pointer's global position, in the layout's units; `None` where
    /// the platform does not tell Pane where the pointer is outside its
    /// own windows.
    pub pointer: Option<Point>,
    /// The display of the operating system's active window — the one the
    /// user is working in, whatever application owns it; `None` where the
    /// platform does not tell Pane.
    pub active: Option<DisplayId>,
}

impl DisplayLayout {
    /// The display with identity `id`, if the layout lists one.
    pub fn display(&self, id: DisplayId) -> Option<&Display> {
        self.displays.iter().find(|display| display.id == id)
    }

    /// The display `point` lies on, if any.
    fn containing(&self, point: Point) -> Option<&Display> {
        self.displays
            .iter()
            .find(|display| display.bounds.contains(point))
    }

    /// The display a fallback lands on: the primary, or the first
    /// display there is, so a layout that names no primary still has an
    /// available display to open on.
    fn fallback(&self) -> Option<&Display> {
        self.primary
            .and_then(|primary| self.display(primary))
            .or_else(|| self.displays.first())
    }
}

/// What the choice and the layout resolve to: the display the launcher
/// opens on, and why that is not the display the choice names, when it is
/// not.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    /// The display the launcher opens on.
    pub display: Display,
    /// Why the chosen display is not used, if the resolution fell back:
    /// the display is disconnected or unknown, or the system does not
    /// tell Pane what the choice needs. The Launcher page shows this
    /// beside the choice; the opening that fell back is still placed on
    /// an available display.
    pub fallback: Option<String>,
}

/// Resolves `choice` against `layout`: the display the launcher window
/// opens on. The chosen display is used when the layout has it; a choice
/// whose answer the system does not give (`None`), or whose display is
/// gone, falls back to the primary display, or the first one there is, so
/// the launcher opens with its controls reachable rather than off-screen
/// — and says it fell back. `None` only when the layout has no display
/// at all, in which case there is nothing to place the window on.
pub fn resolve(layout: &DisplayLayout, choice: OpeningMonitor) -> Option<Resolved> {
    let chosen = match choice {
        OpeningMonitor::Primary => layout.primary,
        OpeningMonitor::Pointer => layout
            .pointer
            .and_then(|pointer| layout.containing(pointer).map(|display| display.id)),
        OpeningMonitor::ActiveWindow => layout.active,
    };
    let chosen = chosen.filter(|id| layout.display(*id).is_some());
    match chosen {
        Some(id) => Some(Resolved {
            display: layout.display(id).cloned()?,
            fallback: None,
        }),
        None => {
            let display = layout.fallback()?.clone();
            Some(Resolved {
                display,
                fallback: Some(fallback_reason(choice)),
            })
        }
    }
}

/// What the resolution says when the choice's display cannot be used: the
/// choice, what it asked for, and what Pane does instead.
fn fallback_reason(choice: OpeningMonitor) -> String {
    let named = match choice {
        OpeningMonitor::Primary => "the primary display",
        OpeningMonitor::Pointer => "the display the pointer is on",
        OpeningMonitor::ActiveWindow => "the display of the window you are working in",
    };
    format!(
        "Pane cannot open on {named}: it is not connected, or this system does not tell \
         Pane where it is. The launcher opens on the primary display instead."
    )
}

#[cfg(test)]
mod tests {
    use super::{Display, DisplayId, DisplayLayout, Point, Rect, Size, resolve};
    use crate::host_settings::OpeningMonitor;

    /// A display with identity `id`, covering the square from (`x`, `y`)
    /// of the given size, whose usable area is inset by the same amount
    /// on every side.
    fn display(id: u64, x: f32, y: f32, width: f32, height: f32, inset: f32) -> Display {
        Display {
            id: DisplayId(id),
            bounds: rect(x, y, width, height),
            usable: rect(
                x + inset,
                y + inset,
                width - 2. * inset,
                height - 2. * inset,
            ),
        }
    }

    /// A rectangle from its origin and size.
    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            origin: Point { x, y },
            size: Size { width, height },
        }
    }

    /// A two-display layout: the primary on the left, a second display
    /// to its right, with the pointer and the active window where the
    /// test puts them.
    fn layout(pointer: Option<Point>, active: Option<u64>) -> DisplayLayout {
        DisplayLayout {
            displays: vec![
                display(1, 0., 0., 1920., 1080., 40.),
                display(2, 1920., 0., 2560., 1440., 60.),
            ],
            primary: Some(DisplayId(1)),
            pointer,
            active: active.map(DisplayId),
        }
    }

    #[test]
    fn the_primary_choice_opens_on_the_primary_display() {
        let resolved = resolve(&layout(None, None), OpeningMonitor::Primary).unwrap();
        assert_eq!(resolved.display.id, DisplayId(1));
        assert_eq!(resolved.fallback, None);
    }

    #[test]
    fn the_pointer_choice_opens_on_the_display_the_pointer_is_on() {
        // On the second display, including at its very edge. The edge the
        // two displays share belongs to both; the first in the layout
        // wins it, as the primary is listed first.
        for (x, y) in [(2500., 700.), (1921., 1.), (4479., 1439.)] {
            let resolved =
                resolve(&layout(Some(Point { x, y }), None), OpeningMonitor::Pointer).unwrap();
            assert_eq!(resolved.display.id, DisplayId(2), "the pointer at {x},{y}");
            assert_eq!(resolved.fallback, None);
        }
        // On the primary.
        let resolved = resolve(
            &layout(Some(Point { x: 100., y: 100. }), None),
            OpeningMonitor::Pointer,
        )
        .unwrap();
        assert_eq!(resolved.display.id, DisplayId(1));
    }

    #[test]
    fn the_active_window_choice_opens_on_its_display() {
        let resolved = resolve(&layout(None, Some(2)), OpeningMonitor::ActiveWindow).unwrap();
        assert_eq!(resolved.display.id, DisplayId(2));
        assert_eq!(resolved.fallback, None);
    }

    #[test]
    fn a_choice_the_system_does_not_answer_falls_back_to_the_primary() {
        for choice in [OpeningMonitor::Pointer, OpeningMonitor::ActiveWindow] {
            let resolved = resolve(&layout(None, None), choice).unwrap();
            assert_eq!(
                resolved.display.id,
                DisplayId(1),
                "{choice:?} fell back to an available display"
            );
            assert!(
                resolved
                    .fallback
                    .as_deref()
                    .is_some_and(|reason| { reason.contains("this system does not tell Pane") }),
                "{choice:?} says why: {:?}",
                resolved.fallback
            );
        }
    }

    #[test]
    fn a_pointer_on_no_display_falls_back() {
        // Outside every display's bounds: the layout does not guess which
        // display the pointer is nearest.
        let resolved = resolve(
            &layout(Some(Point { x: 5000., y: 500. }), None),
            OpeningMonitor::Pointer,
        )
        .unwrap();
        assert_eq!(resolved.display.id, DisplayId(1));
        assert!(resolved.fallback.is_some());
    }

    #[test]
    fn a_disconnected_or_unknown_display_falls_back() {
        // The record holds a display the layout no longer lists.
        let layout = DisplayLayout {
            displays: vec![display(9, 0., 0., 1920., 1080., 40.)],
            primary: Some(DisplayId(9)),
            pointer: None,
            active: Some(DisplayId(4)),
        };
        let resolved = resolve(&layout, OpeningMonitor::ActiveWindow).unwrap();
        assert_eq!(resolved.display.id, DisplayId(9));
        assert!(
            resolved
                .fallback
                .as_deref()
                .is_some_and(|reason| reason.contains("it is not connected")),
            "the reason names the fallback: {:?}",
            resolved.fallback
        );
    }

    #[test]
    fn with_no_primary_the_first_display_is_the_fallback() {
        let layout = DisplayLayout {
            displays: vec![display(2, 0., 0., 1920., 1080., 40.)],
            primary: None,
            pointer: None,
            active: None,
        };
        let resolved = resolve(&layout, OpeningMonitor::Primary).unwrap();
        assert_eq!(resolved.display.id, DisplayId(2));
    }

    #[test]
    fn no_displays_at_all_resolve_to_none() {
        let layout = DisplayLayout::default();
        assert!(resolve(&layout, OpeningMonitor::Primary).is_none());
    }

    #[test]
    fn a_window_opens_centered_in_the_usable_area_and_within_it() {
        let display = display(2, 1920., 0., 2560., 1440., 60.);
        // The usable area runs 1980 to 4420 across and 60 to 1380 down.
        // A window that fits: centered in it.
        let bounds = display.window_bounds(Size {
            width: 760.,
            height: 460.,
        });
        assert_eq!(bounds.origin, Point { x: 2820., y: 490. });
        // A window exactly as tall as the area: centered is its top, and
        // the window ends at its bottom — every part of it, its controls
        // included, is on the display.
        let bounds = display.window_bounds(Size {
            width: 760.,
            height: 1320.,
        });
        assert_eq!(bounds.origin, Point { x: 2820., y: 60. });
        assert_eq!(
            bounds.origin.y + bounds.size.height,
            1380.,
            "the window ends inside the usable area"
        );
        // A window larger than the usable area sits at its top left: the
        // most the display can show of it.
        let bounds = display.window_bounds(Size {
            width: 3000.,
            height: 1600.,
        });
        assert_eq!(bounds.origin, Point { x: 1980., y: 60. });
    }

    #[test]
    fn a_point_on_the_edge_is_on_the_display() {
        let display = display(1, 0., 0., 1920., 1080., 40.);
        assert!(display.bounds.contains(Point { x: 1920., y: 1080. }));
        assert!(!display.bounds.contains(Point {
            x: 1920.5,
            y: 1080.
        }));
    }
}
