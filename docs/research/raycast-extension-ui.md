# Raycast extension UI and implications for JS/TS plus Rust

Checked 2026-09-27 against official API documentation. This is documentation research, not a desktop runtime audit. The user subsequently accepted Pi-style standard controls plus custom views and selected GPUI CE in [ADR 0003](../adr/0003-gpui-ce-and-extensible-views.md). JS/TS and Rust authoring at launch are accepted. The implementation recommendations below remain proposals.

Raycast extensions describe interfaces using React and supported host components: List, Grid, Detail, Form and actions/keyboard shortcuts. Authors can compose their own React components from those elements and manage application state, while Raycast controls the rendered primitives. [UI API](https://developers.raycast.com/api-reference/user-interface)

React here does not mean an embedded website. Raycast's FAQ explicitly excludes HTML/CSS and react-dom: a custom reconciler converts the React tree to a host render tree. Its AppKit explanation is macOS-specific and does not establish the Windows implementation. The reviewed documentation does not expose a general-purpose DOM, canvas or WebView component for arbitrary embedded web interfaces. [FAQ](https://developers.raycast.com/misc/faq)

Store guidance also requires the host navigation API instead of a custom navigation stack. This is a publication policy as well as evidence of a deliberately consistent interaction model. [Store preparation](https://developers.raycast.com/basics/prepare-an-extension-for-store#navigation)

## Design recommendation, not an accepted implementation

Provide common launcher views and actions through a language-neutral view/event contract. A JS/TS SDK could offer React-style authoring; a Rust SDK could offer typed builders. Both would submit view descriptions and receive identifiable events. Rust authors should not need to author a separate JavaScript UI merely to show a list or form. This does not choose JSON, WIT, native IPC or a renderer.

Custom views are a separate choice: allow richer composition/drawing primitives or an optional embedded web surface. Preserve an extension point in the design, but determine the actual custom-view API and launch scope with the user. A WebView can introduce startup, memory and platform costs that have not been measured here. Host-rendered standard views alone do not establish a low-memory launcher.
