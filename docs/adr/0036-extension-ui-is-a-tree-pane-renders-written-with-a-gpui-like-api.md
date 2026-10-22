# Extension UI is a tree Pane renders, written with a GPUI-like API

Accepted 2026-10-06 by the user's decision, after the [Raycast deep dive](../research/raycast-deep-dive.md#extension-ui-bridge) (its decision 1). It settles the shared view, event and drawing API that [ADR 0003](0003-gpui-ce-and-extensible-views.md) left open (Q13; [current decisions](../current-decisions.md) item 2), within ADR 0003's choice of GPUI CE with standard controls and custom interactive views. The command's list, the [forms](../forms.md) of #20 and the [custom view](../custom-views.md) of #21 are rebuilt on it. A list becomes the standard List view, a form the standard Form, and a custom view's shapes a canvas inside the tree. Of the research's two wire formats, it takes versioned JSON over a typed flat WIT arena. It also overrules the research's suggestion that styling be tokens only: raw values are allowed too.

So an extension describes its UI as a tree of nodes, and Pane renders and runs it. Authors never see the wire format, because the SDKs hide it. In Rust they write a builder shaped like GPUI's; in JavaScript and TypeScript, JSX:

```rust
column().gap(Space::M).children([
    text("Focus").style(TextStyle::Title),
    button("Pause").on_click(cx.listener(|this, _, cx| this.pause(cx))),
])
```

```tsx
<Column gap="m">
  <Text style="title">Focus</Text>
  <Button onClick={pause}>Pause</Button>
</Column>
```

Underneath, the tree is versioned JSON carried through a typed WIT envelope. `render` answers with the tree, which names the version of the component set it uses. `handle-event` takes the id of a callback the tree named, with the event's details, and Pane then asks for the tree again. A new component therefore needs no WIT change. A node Pane does not know degrades gracefully: Pane draws the fallback the author gave, or its children, and does not fail the view. The component set is versioned on the terms of [ADR 0010](0010-best-effort-extension-api-compatibility.md).

**What the tree holds.** Layout primitives: row, column, stack, scroll, grow and shrink, and wrap. Pane's shared components are first-class nodes, the same ones Pane's own UI draws: rich row, icon tile, keycap, tag, button, input, select, toggle, Markdown and others. A canvas is a leaf node for drawing of the extension's own, and #21's custom view becomes one. The standard views are built from these nodes and come with Raycast's capabilities. List has sections, accessories, an empty view, a loading state, a dropdown and pagination. Detail is Markdown with metadata. Grid is the third. Form has more field types than #20's.

**Styling.** Theme tokens (tone, text style, space) are the easy default, and they follow the user's appearance and background image. For as much flexibility as authors want, raw values (hex colours, pixel sizes) are allowed too. Pane corrects a colour's contrast against what it is drawn on, so a raw value cannot make text unreadable.

**Icons.** An icon can be:

- one of a built-in set, by name (Pane vendors the whole MIT-licensed reicon set, which its own UI already draws from);
- a packaged PNG or SVG, with automatic `@dark` and `@light` variants or an explicit light and dark pair;
- a web image by URL, which the host downloads within its existing HTTP limits and caches, showing a fallback while it loads or if it fails;
- a file's or an application's own system icon, by path.

Every icon accepts a tint, a mask (circle or rounded rectangle) and a fallback. The tint can be a token, a light and dark pair, or a raw colour, contrast-corrected. The SDKs add helpers for favicons, initials avatars and progress rings. An extension's own icon is the `icon` field of `pane.json`, and a command can set its own, falling back to the extension's. A published package must have one, 512 × 512. A local or development package without one gets a generated first-letter tile.

**What the host owns.** Pane reconciles each new tree against the last by the stable keys the author gives its nodes. It keeps each key's state: text editing, input-method composition, focus, hover and pressed states, scroll position and the accessible representation. Inputs are partially controlled. Pane edits the text at once and tells the extension, and a value the extension sets wins. A render answer may ask to be rendered again after a number of milliseconds, for timers and polling, without the runtime pushing anything. Views form a navigation stack: an extension pushes and pops views, Escape pops one, and the extension hears when a view of its own is popped.

Real push is a later, larger slice: an extension re-rendering by itself, for example when its own work finishes. It needs a runtime that serves instances concurrently. Today the runtime calls one guest at a time, and a guest runs only inside a call from Pane.

ADR 0003 asks for standard controls and custom views in JavaScript, TypeScript and Rust alike. Raycast offers only a fixed set of leaves and no drawing, so Pane goes beyond it. The prior art the research read converges on this shape: the host renders a component set the guest arranges and owns whatever must react within a frame, authors give stable keys, and components are versioned. That prior art includes Shopify's remote-dom, Figma widgets, Flutter's rfw and others. Keeping the tree out of WIT's types keeps the contract stable as components are added; today a change of any exported type forces every component to be rebuilt ([current decisions](../current-decisions.md) item 5). A builder shaped like GPUI's matches how Pane's own UI is written, so Pane and its authors share one vocabulary. The user chose raw values beside tokens because authors should be as free as possible.
