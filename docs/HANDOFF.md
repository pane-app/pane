# Pane handoff

Updated 2026-10-10. This handoff covers Pane, the cross-platform launcher and its extension system.

The project is published at [pane-app/pane](https://github.com/pane-app/pane). The repository contains a launcher under development with native smoke evidence for its slices; there is a package a clean machine installs on Linux ([#53](https://github.com/pane-app/pane/issues/53)) and Windows ([#51](https://github.com/pane-app/pane/issues/51)) ([installer](installer.md)), whose first setup installs the default extensions from the commits of their own repositories' release tags that the Pane release pins ([ADR 0045](adr/0045-official-extensions-live-in-their-own-repositories.md)); the artifact source that serves Pane's own application updates is still not deployed, and no published installer exists yet.

## Start here

1. Read [where the work stands](#where-the-work-stands) below, then [current decisions](current-decisions.md) for the reconciled Q1-Q41 index and runtime status as of 2026-09-28. Decisions made since are in the [ADRs](adr/) from [0026](adr/0026-host-keeps-quick-slots-by-identity.md) on, and the specifications listed below.
2. Read [extension policies](extension-policy-proposal.md) for detailed accepted behavior.
3. Use [the interview](launcher-design-interview.md) and relevant [ADRs](adr/) for rationale and history. Earlier proposals inside the interview are historical, not competing current instructions.
4. See [record limits](current-decisions.md#record-limits) for the limits of the saved discussion and evidence.

## Where the work stands

All work is tracked in [GitHub Issues](https://github.com/pane-app/pane/issues); open [specifications](https://github.com/pane-app/pane/issues?q=is%3Aissue+is%3Aopen+label%3Aspecification) and their native sub-issues are the current plan, and this section only summarizes them.

- The [Pane specification #1](https://github.com/pane-app/pane/issues/1) and its implementation issues are closed: the extension host, the default extensions, local/npm/Git installation, development mode, pausing, schedules, services and the installer packages are on `main`.
- [Settings and shortcuts #70](https://github.com/pane-app/pane/issues/70) is closed and on `main`.
- [Launcher UI #61](https://github.com/pane-app/pane/issues/61) is open for its macOS and Linux halves ([#66](https://github.com/pane-app/pane/issues/66), [#67](https://github.com/pane-app/pane/issues/67)) and the combined release validation ([#68](https://github.com/pane-app/pane/issues/68)).
- [The Windows UI port #90](https://github.com/pane-app/pane/issues/90) (#91-#102) and the Raycast-style redesign that followed it are on `main`, landed per ticket, with the branch's original commits kept under the tag `archive/impl-ui-91-2026-10-06`. The redesign is recorded in [ADR 0027](adr/0027-quick-slots-are-an-ordered-list.md) (pins as an ordered list) and [ADR 0028](adr/0028-the-launcher-draws-a-background-image-the-user-chooses.md) (a background image); parts of it supersede #90's contract. [ADR 0029](adr/0029-the-ui-is-accepted-as-built.md) accepted the UI as built and retired the reference comparison, so #103's integrated acceptance and the comparison's re-recording ([#107](https://github.com/pane-app/pane/issues/107)) are closed as not planned. [#105](https://github.com/pane-app/pane/issues/105) records that redesign; its one open verification is the human real-window ticket [#108](https://github.com/pane-app/pane/issues/108).
- [Official extensions #207](https://github.com/pane-app/pane/issues/207) is nearly done: the five default extensions (calculator, applications, quicklinks, files, clipboard history) live in their own public repositories in the [`pane-app`](https://github.com/pane-app) organization, each built and released there ([#279](https://github.com/pane-app/pane/issues/279)), and each Pane release pins the commit of each repository's release tag it was tested with ([#282](https://github.com/pane-app/pane/issues/282), [`crates/pane/defaults.json`](../crates/pane/defaults.json), [ADR 0045](adr/0045-official-extensions-live-in-their-own-repositories.md)). First setup fetches those pinned commits with Pane's own Git client and installs them with their default identities ([#278](https://github.com/pane-app/pane/issues/278)); Settings marks official extensions as Pane's own wherever it lists them ([#280](https://github.com/pane-app/pane/issues/280)); the extensions' sources have left this repository's `guests/` tree, the samples standing in as fixtures ([#285](https://github.com/pane-app/pane/issues/285)); the smokes install the defaults the way a release does, from their repositories cloned at the pinned commits and served on this computer ([#284](https://github.com/pane-app/pane/issues/284)). Still open: the first-setup choice screen ([#283](https://github.com/pane-app/pane/issues/283), deferred), publishing the SDKs — a person's step, never CI's ([#281](https://github.com/pane-app/pane/issues/281)), building the repositories' components against the published `pane-extension` crate ([#287](https://github.com/pane-app/pane/issues/287)), and default-extension updates from their repositories' newer release tags ([#269](https://github.com/pane-app/pane/issues/269)).

Branch CI tiers and pacing are in [agents/ci.md](agents/ci.md).

## Current direction

- **Pane** is the product and workspace name (early research spikes under `research/` still say `kyoko`).
- Small, general-purpose, extensible launcher targeting Windows/macOS/Linux, using **GPUI CE** (a pinned fork; see [gpui-fork](gpui-fork.md)). Contributors on all three systems have native build/run/test workflows, and CI builds and tests on all three; staggered previews remain accepted.
- Trusted extensions, capability priority, own SDK and npm/Git/local distribution. JS/TS **and Rust** at launch; Python/C# later.
- **WASI 0.3 required.** WIT/Wasmtime is the current architecture under evaluation. Earlier Node workers/native Rust entry-point choices are superseded as the implementation baseline; optional prebuilt native helpers remain accepted.
- **QuickJS is provisional**, with temporary integration patches, unresolved snapshot random-state initialization, and no production backend commitment.
- Default app launching, calculator, quicklinks, file search and optional clipboard history are disableable. AI belongs in extensions.
- Internet-first: the installer carries no default extensions; first setup fetches them from the commits of their repositories' release tags that the Pane release pins (ADR 0045). Ordinary supported-package users install no runtime/compiler manually.
- Compatible extension updates are automatic with controls; Pane application updates notify the user, who chooses installation.
- Automatic UI recovery skips an identified broken extension and notifies the user: since #16 a package that cannot start or crashes three times within five minutes is [paused](pausing.md) with Retry, across restarts. Shared runtime crashes may have no identifiable single culprit (#17). Since #18 a guest computing for 5 seconds in all without finishing (its own computing only, never Pane's host calls or waiting) is stopped and counted towards pausing its package, a native helper ran for at most 30 seconds until #136 removed that limit (other calls are served while one waits), and a runtime thread whose heartbeat stops is said to be not responding yet after 10 seconds, then given up on after 30 and replaced, naming no extension ([pausing](pausing.md#when-an-extension-stops-responding)).
- Licensing follows Zed's direction: the application is GPL-3.0-or-later, and the extension contract and guests are Apache-2.0 OR MIT ([README](../README.md#licensing), [audit](research/licensing-audit.md)). This does not prohibit compliant paid forks.

Detailed identity, dependency, disable/data, API stability, search, reload and update decisions are in the [decision index](current-decisions.md). Do not replace them with generic assumptions about Pi, Raycast or VS Code.

## Evidence and limits

The [validation checkpoint](research/wasi03-validation.md) links the Rust, JS and GPUI experiments.

- [Rust std](research/p3-std-spike/README.md): P3-only filesystem/environment/clock execution with pinned source-built toolchain.
- [QuickJS port](research/qjs-p3-port-spike/README.md): real libraries, async streams and 20 sequential calls pass; source patch and raw results saved.
- [Embedded host](research/p3-only-host/README.md): P3-only registration; stock mixed JS rejected.
- [GPUI Windows probe](research/wasi03-gpui-spike/README.md): guest JSON rendering, JSON replacement and clicks. Not live guest hot reload or production IPC.

Saving a developed local package builds and reloads it since #12/#13 ([development mode](development-mode.md)). Disabling, reloading or updating an extension stops its pending calls since #14 ([generations](generations.md)); remaining validation includes runtime reseeding, time limits on waiting and user cancellation, true concurrency, host/UI integration and macOS/Linux. The #4 resource workload and /proc sampler exist and run in CI's Linux leg ([measurement record](research/resource-measurements.md)); their numbers and targets are still pending. Benchmark numbers are scoped CLI/prototype measurements, not product budgets.

Tool locations and scratch paths are recorded with the probes, including [P3 tool metadata](research/p3-native-toolchain-local.json). Some compiler/toolchain/checkouts and built binaries live under Windows temporary storage. Their presence is not a durable source-control guarantee. Repository source scripts, reports and the patch are the recovery path; runtime/toolchain installation must not silently alter the user's global Rust default.

## Workflow configuration

Both [AGENTS.md](../AGENTS.md) and [CLAUDE.md](../CLAUDE.md) are present by explicit user choice. Shared configuration:

- [GitHub issue tracker](agents/issue-tracker.md).
- [Default triage labels](agents/triage-labels.md).
- [Single-context domain documentation](agents/domain.md).

Next: work open, unassigned implementation issues whose native blockers and stated prerequisites are complete, in their specification's order ([issue tracker](agents/issue-tracker.md)). Platform evidence remains independent; `ready-for-agent` does not clear blockers or authorize a release.

## Historical records

The interview, research reports and superseded ADRs retain Pane's design history. Use the current decision index for the accepted direction. Earlier notes referenced pre-audit snapshots, but those snapshots are absent from this repository; do not rely on them as recoverable evidence.
