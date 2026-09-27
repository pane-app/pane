# Use GPUI CE with standard controls and custom extension views

Accepted 2026-09-27. The user chose [GPUI CE](https://github.com/gpui-ce/gpui-ce) as the launcher renderer and Pi's UI extensibility pattern: convenient standard controls plus custom interactive components/views. Apply that pattern to a desktop interface; Pi's terminal rendering implementation is not the launcher UI.

JavaScript/TypeScript and Rust extension authoring are launch requirements. The extension UI interface must accommodate both, while the GPUI CE integration is Rust-based. Selecting GPUI CE does not select a JavaScript runtime, extension transport, Wasm requirement, dynamic-library ABI or concrete reload mechanism. Those choices must preserve the capability-first trust decision in [ADR 0002](0002-trusted-extensions-and-open-distribution.md) and the accepted disable/data behavior.

GPUI CE exposes both declarative views and lower-level custom elements, making it a relevant renderer for this direction. The cost of choosing it includes maintaining our extension-to-renderer integration and validating desktop behavior on Windows, macOS and Linux. Low idle resource usage, hot reload and a stable public extension SDK remain requirements to verify, not properties established by choosing the framework.
