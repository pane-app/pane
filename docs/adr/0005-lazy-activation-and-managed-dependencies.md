# Activate extensions when needed and manage their runtime setup

**Subsequent setup choice:** Q36 prefers internet-first acquisition of runtime/default-feature payloads instead of bundling them in the installer. The no-manual-tools promise remains. WASI3 components plus optional prebuilt helpers are the current direction under evaluation; exact packaging remains open. See [current decisions](../current-decisions.md).

**Note (#11):** reloading a package starts each of its available commands at once to report a startup failure with Retry; this is a deliberate, provisional exception to lazy activation, recorded in [current decisions](../current-decisions.md).

Accepted 2026-09-27 for Q15-Q16. Extensions support on-demand commands/events, scheduled tasks and explicit continuing background services; installing an extension does not by itself require keeping its executable code active. Enabled background work follows the accepted disable/reload lifecycle, preserving capability while avoiding unnecessary startup and idle work.

Ordinary users should install the launcher and supported extension packages without manually installing Node, Rust, npm, Git, compilers or other development tools. The launcher must bundle or manage its runtime and installation dependencies, with prebuilt Rust packages for supported targets if native Rust execution is selected. Pi-style npm/Git/local sources remain supported; developer source builds may need toolchains, but source-only packages must not silently turn the normal installation flow into manual environment setup.

This trades additional packaging, runtime maintenance and platform builds for a simpler user experience. The precise JS runtime, Rust execution format, dependency acquisition mechanism and offline guarantees remain open. It does not remove account setup or the functional dependencies of integrations with external applications/services. See [extension policies](../extension-policy-proposal.md).
