# Pi extension UI: Q13 evidence

Research date: 2026-09-27. Sources are current upstream documentation and examples. The user subsequently accepted Pi-style standard controls plus custom views and selected GPUI CE in [ADR 0003](../adr/0003-gpui-ce-and-extensible-views.md); concrete implementation recommendations remain open.

Pi combines convenient UI helpers with custom terminal components. Its extension UI is more flexible than a fixed collection of screens, but the rendering contract is specifically for terminals.

## Built-in and custom UI

The built-in helpers cover selection, confirmation, text input, multiline editing, notifications, status text, and persistent widgets. Extensions can replace the header, footer, and main editor; register custom tool/session-entry renderers; and open a custom screen or overlay through `ctx.ui.custom()`. The TUI package supplies reusable layout, selection, input, scrolling, Markdown, and image components. Custom components render terminal lines, can handle input, and participate in the host's invalidation/render lifecycle. Overlay positioning and focus are configurable. These are terminal primitives, not a browser DOM or a native desktop-widget API. [Terminal UI documentation](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/tui.md)

Concrete examples establish that this is real extensibility:

- The [modal editor example](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/examples/extensions/modal-editor.ts) subclasses `CustomEditor`, changes editing key behavior, and retains application shortcuts through the base implementation.
- The [custom footer example](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/examples/extensions/custom-footer.ts) replaces the footer, subscribes to branch changes, and supplies a disposal function.
- The [DOOM overlay example](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/examples/extensions/doom-overlay/index.ts) opens a continuously rendered game component with `ctx.ui.custom(..., { overlay: true })`. Its comment describes 35 FPS; this research did not benchmark it.

## Important boundary: interactive terminal versus other clients

Pi's complete UI works in interactive terminal mode. RPC clients can implement a supported interaction protocol, but `custom()` returns `undefined`; editor/header/footer replacement and direct terminal input are unavailable. RPC widgets accept text lines rather than custom component factories. JSON and print modes have no UI. [Extension modes](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md#ui-and-modes), [RPC extension UI protocol](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/rpc-extension-ui.md)

Pi extensions run as trusted TypeScript/JavaScript code in the Pi process. That permits external integrations, but arbitrary desktop windows launched by extension code would be external integrations, not portable embedded UI supplied by Pi's terminal component contract. [Extensions documentation](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md)

## Implications for this launcher (inferences, not decisions)

- Pi demonstrates a useful pattern: common host-owned controls plus a documented custom-rendering escape hatch.
- Copying that pattern into a desktop launcher requires choosing our own desktop rendering contract. Pi's TUI API cannot directly provide embedded desktop custom views.
- First-class JS/TS and Rust support suggests keeping standard view descriptions and UI events language-neutral, with ergonomic bindings in each SDK. A custom-view contract must separately define what Rust can produce; a JS-only component interface does not establish Rust UI support.
- Providing custom views does not itself require arbitrary HTML/webviews. Whether the escape hatch uses a layout/component tree, custom drawing, or web content remains a product/runtime decision, with memory and implementation costs still unmeasured.
