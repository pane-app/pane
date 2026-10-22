# Require WASI 0.3 for the extension architecture

Accepted 2026-09-27: the user explicitly requires "wasi3 ... not wasi2" while evaluating WIT and Wasmtime. Target WASI 0.3 interfaces and component-native async; the prior Rust WASIp2 and synchronous JS experiments remain historical feasibility evidence, not validation of this requirement. Toolchain selection must demonstrate actual 0.3 imports/exports and execution; do not silently substitute WASI 0.2 or label mixed 0.2/0.3 output as a completed migration.

Exact tool versions and the 0.3 patch version remain implementation choices to verify together. GPUI CE, JS/TS and Rust authoring, trusted capabilities and managed end-user setup remain required; SDK/backend completeness and optional native helpers (Q33) are not settled by this decision. See [toolchain findings](../research/wasi03-requirement.md).
