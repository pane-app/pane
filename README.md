# Pane

A small, extensible desktop launcher for Windows, macOS and Linux, inspired by Raycast and Pi.

Pane is currently in planning and prototype validation. There is no installable application yet. The intended UI uses GPUI CE, with JavaScript, TypeScript and Rust extensions through a WASI 0.3 component interface. Runtime/backend choices still require validation.

## Project documents

- [Specification](https://github.com/hoangvu12/pane/issues/1)
- [Implementation tickets](https://github.com/hoangvu12/pane/issues?q=is%3Aissue+label%3Aimplementation)
- [Cross-platform contributor requirements](https://github.com/hoangvu12/pane/issues/1#cross-platform-contributor-requirement)
- [Current decisions](docs/current-decisions.md)
- [Handoff and evidence limits](docs/HANDOFF.md)
- [Domain vocabulary](CONTEXT.md)

Contributors on all three operating systems must have working build/run/test workflows early in development. Existing prototype evidence covers Windows only; macOS and Linux support remains to be validated. Research scripts may depend on the temporary toolchains and local paths documented alongside them.

The 52 implementation issues are published; work starts when each issue's blockers are complete. Research results and prototype source are under `docs/research/`; they are not production support guarantees.

The provisional licensing direction is GPL-3.0-or-later for the application, with the exact component/SDK split still unresolved. A project license has not yet been applied. Existing third-party notices and licenses remain applicable.
