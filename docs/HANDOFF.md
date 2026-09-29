# Pane handoff

Updated 2026-09-28. This handoff covers Pane, the cross-platform launcher and its extension system.

The project is published at [hoangvu12/pane](https://github.com/hoangvu12/pane). The repository contains planning documents and research prototypes; there is no installable Pane application yet.

## Start here

1. Read [current decisions](current-decisions.md) for the reconciled Q1-Q41 index, runtime status and open work.
2. Read [extension policies](extension-policy-proposal.md) for detailed accepted behavior.
3. Use [the interview](launcher-design-interview.md) and relevant [ADRs](adr/) for rationale and history. Earlier proposals inside the interview are historical, not competing current instructions.
4. See [record limits](current-decisions.md#record-limits) for the limits of the saved discussion and evidence.

The [Pane specification](https://github.com/hoangvu12/pane/issues/1) and [52 implementation issues](https://github.com/hoangvu12/pane/issues?q=is%3Aissue+label%3Aimplementation) are tracked in GitHub Issues. The specification includes the accepted cross-platform contributor requirement; native macOS/Linux sample builds and runs ([#7](https://github.com/hoangvu12/pane/issues/7), [#8](https://github.com/hoangvu12/pane/issues/8)) precede broad shared features. [Planning prerequisites](https://github.com/hoangvu12/pane/issues/1#planning-prerequisites) and [release validation](https://github.com/hoangvu12/pane/issues/57) are separate issues. The proposed testing boundary remains pending feedback; migration does not complete implementation or validate runtime behavior.

## Current direction

- **Pane** is the chosen product name; the existing workspace is still named `kyoko`.
- Small, general-purpose, extensible launcher targeting Windows/macOS/Linux, using **GPUI CE**. Contributors on all three systems need working native build/run/test workflows early. Only Windows prototypes have been tested; staggered previews remain accepted, with the early contributor baseline required before shared feature expansion.
- Trusted extensions, capability priority, own SDK and npm/Git/local distribution. JS/TS **and Rust** at launch; Python/C# later.
- **WASI 0.3 required.** WIT/Wasmtime is the current architecture under evaluation. Earlier Node workers/native Rust entry-point choices are superseded as the implementation baseline; optional prebuilt native helpers remain accepted.
- **QuickJS is provisional**, with temporary integration patches, unresolved snapshot random-state initialization, and no production backend commitment.
- Default app launching, calculator, quicklinks, file search and optional clipboard history are disableable. AI belongs in extensions.
- Internet-first runtime/default-feature acquisition is preferred over bundling their payloads in the installer. Ordinary supported-package users install no runtime/compiler manually.
- Compatible extension updates are automatic with controls; Pane application updates notify the user, who chooses installation.
- Automatic UI recovery skips an identified broken extension and notifies the user: since #16 a package that cannot start or crashes three times within five minutes is [paused](pausing.md) with Retry, across restarts. Shared runtime crashes may have no identifiable single culprit (#17). Since #18 a guest computing for 5 seconds in all without finishing (its own computing only, never Pane's host calls or waiting) is stopped and counted towards pausing its package, a native helper runs for at most 30 seconds (provisional), and a runtime thread whose heartbeat stops is said to be not responding yet after 10 seconds, then given up on after 30 and replaced, naming no extension ([pausing](pausing.md#when-an-extension-stops-responding)).
- Licensing follows Zed's primarily GPL-3.0-or-later direction provisionally; exact component/SDK split and dependency review remain open. This does not prohibit compliant paid forks.

Detailed identity, dependency, disable/data, API stability, search, reload and update decisions are in the [decision index](current-decisions.md). Do not replace them with generic assumptions about Pi, Raycast or VS Code.

## Evidence and limits

The [validation checkpoint](research/wasi03-validation.md) links the Rust, JS and GPUI experiments.

- [Rust std](research/p3-std-spike/README.md): P3-only filesystem/environment/clock execution with pinned source-built toolchain.
- [QuickJS port](research/qjs-p3-port-spike/README.md): real libraries, async streams and 20 sequential calls pass; source patch and raw results saved.
- [Embedded host](research/p3-only-host/README.md): P3-only registration; stock mixed JS rejected.
- [GPUI Windows probe](research/wasi03-gpui-spike/README.md): guest JSON rendering, JSON replacement and clicks. Not live guest hot reload or production IPC.

Saving a developed local package builds and reloads it since #12/#13 ([development mode](development-mode.md)). Disabling, reloading or updating an extension stops its pending calls since #14 ([generations](generations.md)); remaining validation includes runtime reseeding, time limits on waiting and user cancellation, true concurrency, persistent resource measurements, host/UI integration and macOS/Linux. Benchmark numbers are scoped CLI/prototype measurements, not product budgets.

Tool locations and scratch paths are recorded with the probes, including [P3 tool metadata](research/p3-native-toolchain-local.json). Some compiler/toolchain/checkouts and built binaries live under Windows temporary storage. Their presence is not a durable source-control guarantee. Repository source scripts, reports and the patch are the recovery path; runtime/toolchain installation must not silently alter the user's global Rust default.

## Workflow configuration

Both [AGENTS.md](../AGENTS.md) and [CLAUDE.md](../CLAUDE.md) are present by explicit user choice. Shared configuration:

- [GitHub issue tracker](agents/issue-tracker.md).
- [Default triage labels](agents/triage-labels.md).
- [Single-context domain documentation](agents/domain.md).

Next: work open, unassigned implementation issues whose native blockers and stated prerequisites are complete. Start with [#5: one Rust command through the native UI](https://github.com/hoangvu12/pane/issues/5). [JS/TS integration #6](https://github.com/hoangvu12/pane/issues/6) requires both that issue and [backend prerequisite #2](https://github.com/hoangvu12/pane/issues/2). Early native contributor checks follow in [#7](https://github.com/hoangvu12/pane/issues/7) and [#8](https://github.com/hoangvu12/pane/issues/8). [Licensing](https://github.com/hoangvu12/pane/issues/3) and [measured targets](https://github.com/hoangvu12/pane/issues/4) gate the corresponding distribution/readiness claims. Platform evidence remains independent; `ready-for-agent` does not clear blockers or authorize a release.

## Historical records

The interview, research reports and superseded ADRs retain Pane's design history. Use the current decision index for the accepted direction. Earlier notes referenced pre-audit snapshots, but those snapshots are absent from this repository; do not rely on them as recoverable evidence.
