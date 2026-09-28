# Pane: current decisions

Reconciled 2026-09-28 against the saved interview, ADRs, policy document, test reports, and user messages available in the current conversation. This is a decision index, not a new spec or implementation authorization. It does not reconstruct a verbatim pre-compaction transcript.

Use this file for current status; use the linked records for detail and rationale. Later explicit user instructions take precedence. **Accepted** is a product decision, **provisional** is a direction awaiting detail or validation, **superseded** is historical, and **deferred** is outside the initial scope or postponed. An untested requirement can still be accepted; test success does not turn an assistant recommendation into user approval.

## Interview coverage

All Q1-Q41 have entries in the [saved interview](launcher-design-interview.md). The rows below reconcile those entries with later corrections; they are not copies of the original question wording.

| Question | Current status and meaning | Supporting detail |
| --- | --- | --- |
| Q1 | Accepted: general-purpose, extension-driven desktop launcher inspired by Raycast, Tinycast and Pi. | [Small core](adr/0001-small-core.md) |
| Q2 | Accepted: Windows, macOS and Linux targets, with early native contributor build/run/test support on all three. Only Windows prototypes tested; Q37 allows staggered previews. | [Contributor clarification](https://github.com/hoangvu12/pane/issues/1#cross-platform-contributor-requirement), [platform notes](research/platform-specific-extensions-q34.md) |
| Q3 | Accepted: small permanent feature core and low resource usage; AI belongs in extensions. Numerical budgets remain open. | [Small core](adr/0001-small-core.md) |
| Q4 | Early comparison request; not an independent acceptance of Raycast's architecture. Later explicit choices govern. | [Interview](launcher-design-interview.md) |
| Q5 | Flexibility and avoiding unnecessary architectural rework are intent; early unresolved trust discussion was settled by Q9. | [Interview](launcher-design-interview.md) |
| Q6 | Accepted: default app launching, calculator, quicklinks, file search and optional clipboard history, each disableable. | [Small core](adr/0001-small-core.md), [policies](extension-policy-proposal.md) |
| Q7 | Early comparison request; Q10 selects own SDK and npm/Git/local distribution, without unchanged third-party API compatibility. | [Trust/distribution](adr/0002-trusted-extensions-and-open-distribution.md) |
| Q8 | Accepted: consistent platform behavior as practical; explain actual differences. | [Interview](launcher-design-interview.md) |
| Q9 | Accepted: Pi-style trusted code; capability over mandatory hardening/permission gates. Runtime and OS capabilities still constrain execution. | [Trust](adr/0002-trusted-extensions-and-open-distribution.md) |
| Q10 | Accepted: own API, npm/Git/local installation; no compulsory reviewed store. Build hooks are not categorically prohibited. | [Distribution](extension-policy-proposal.md#trust-and-distribution) |
| Q11 | Accepted: reversible disable stops managed work and retains unexpired saved data; cache deletion and uninstall are distinct. | [Data lifecycle](extension-policy-proposal.md#disable-cache-data-and-uninstall) |
| Q12 | Accepted: JS, TS and Rust at launch. Python/C# later. | [Languages](extension-policy-proposal.md#launch-authoring-languages) |
| Q13 | Accepted: GPUI CE, standard controls and custom interactive extension views. Shared view/event/drawing API is not finalized. | [Renderer](adr/0003-gpui-ce-and-extensible-views.md) |
| Q14 | Accepted: save builds/reloads only the affected extension while the launcher stays open; build failure keeps working code. | [Reload](adr/0004-reload-extensions-without-restarting-launcher.md) |
| Q15 | Accepted: lazy command/event activation, scheduled work and explicit background services. Installation does not imply a running instance. | [Activation](adr/0005-lazy-activation-and-managed-dependencies.md) |
| Q16 | Accepted: no manual developer/runtime tools for normal installation of supported packages. Authors/source builds may need tools. | [Setup](adr/0005-lazy-activation-and-managed-dependencies.md) |
| Q17 | Accepted: root results for apps, commands, quicklinks, calculator and enabled files. Online service contents searched inside commands. | [Search](adr/0006-raycast-style-search-with-extension-providers.md) |
| Q18 | Superseded as baseline: managed Node. WIT/Wasmtime reevaluation and explicit WASI3 requirement came later. | [Historical Node ADR](adr/0008-managed-node-for-javascript-extensions.md), [runtime status below](#runtime-direction-and-evidence) |
| Q19 | Superseded as baseline: native Rust extension executables. Q33 permits native helpers behind WASI3 entry points. | [Historical Rust ADR](adr/0007-native-rust-extension-processes.md), [helpers](adr/0014-optional-native-extension-helpers.md) |
| Q20 | Accepted reliability direction: keep Pane usable, preserve data, expose Retry/logs, suppress repeated background failures. Q39 adds automatic UI recovery. | [Failure policy](extension-policy-proposal.md#extension-failures) |
| Q21 | Accepted: automatic compatible extension updates for unpinned published packages, with global/per-extension controls and manual updates; avoid mid-command replacement. | [Extension updates](extension-policy-proposal.md#extension-updates) |
| Q22 | Node-specific acquisition is historical. No-manual-setup intent survives; Q36 selects internet-first runtime/default-feature acquisition. | [Installation](extension-policy-proposal.md#end-user-installation) |
| Q23 | Superseded as baseline: shared Node process with workers. Wasmtime helper topology and fatal-crash attribution remain to validate. | [Historical process ADR](adr/0009-shared-node-helper-with-extension-workers.md) |
| Q24 | Accepted: minimize API churn but allow breaking changes; authors maintain extensions. No indefinite compatibility/takeover obligation. | [API evolution](adr/0010-best-effort-extension-api-compatibility.md) |
| Q25 | Accepted: host-routed, cross-language calls to explicitly published operations with structured arguments/results/errors; UI commands are not automatically headless APIs. | [Composition](adr/0011-extension-call-and-result-api.md) |
| Q26 | Accepted: show/install compatible missing required extensions; optional integrations not auto-installed; honor pins and deliberate disablement. | [Dependencies](extension-policy-proposal.md#dependencies-on-other-extensions) |
| Q27 | Accepted: offer Disable All / Cancel or Uninstall All / Cancel for required dependents; restoring the dependency does not restore dependents automatically. | [Removal](extension-policy-proposal.md#dependencies-on-other-extensions) |
| Q28 | Accepted: reject a second explicit install of the same canonical source identity; ordinary updates remain supported. | [Identity](adr/0012-pi-style-source-identity.md) |
| Q29 | Accepted: npm name without version, normalized Git host/repository without ref, resolved absolute local path. Cross-source copies may coexist. | [Identity](adr/0012-pi-style-source-identity.md) |
| Q30 | Accepted: developers manually manage local and published copies; no automatic swapping, disabling/restoring or data remapping. | [Development copies](extension-policy-proposal.md#development-and-published-copies) |
| Q31 | Accepted: replacement startup failure reports failure with Retry/logs and managed cleanup; no automatic rollback. Build-failure preservation is different. | [Reload](adr/0004-reload-extensions-without-restarting-launcher.md) |
| Q32 | Deferred: automatic historical-version discovery/downgrade picker. Pins and explicit supported version/ref inputs remain. | [Interview](launcher-design-interview.md) |
| Q33 | Accepted: optional prebuilt OS/architecture-specific native helpers through host APIs with managed lifecycle; extension entry point remains WASI3. | [Helpers](adr/0014-optional-native-extension-helpers.md) |
| Q34 | Accepted: simple supported-OS metadata and per-action availability explanations; preserve functioning actions. No general compatibility-rule language. | [Platform policy](extension-policy-proposal.md#platform-specific-extensions-and-actions) |
| Q35 | Deferred: browsable catalog/store decisions until functionality works; external source installation remains. | [Interview](launcher-design-interview.md) |
| Q36 | Accepted preference: installer should omit runtime/default-feature payloads and assume internet. Automatic acquisition/cache is proposed; exact setup timing is open. | [Latest answers](launcher-design-interview.md#latest-answers-q36q41) |
| Q37 | Accepted: staggered previews; Windows-only testing acknowledged. macOS/Linux support is not validated. | [Latest answers](launcher-design-interview.md#latest-answers-q36q41) |
| Q38 | Accepted: notify about Pane updates; user chooses installation. No automatic application download/install/restart was accepted. Separate from Q21. | [Latest answers](launcher-design-interview.md#latest-answers-q36q41) |
| Q39 | Accepted UX direction: automatically detect/skip an identified broken extension and notify through UI, without requiring CLI recovery. Detailed attribution, persistence and retry mechanics are proposed. | [Recovery proposal](launcher-design-interview.md#proposed-automatic-recovery-behavior-for-q39) |
| Q40 | Accepted 2026-09-27 (#3): the application is GPL-3.0-or-later; the WIT contract, guest SDKs, samples and fixtures are Apache-2.0 OR MIT; no additional GPL permission for extensions (no license flows into them through embedded Pane code); contributions under DCO with copyright kept by contributors ("The Pane contributors"). Compliant paid forks remain allowed. Release notice bundles and the source procedure are still to be exercised. | [License research](research/pane-license-options-q40.md), [dependency audit](research/licensing-audit.md) |
| Q41 | Accepted: product name Pane. Kyoko is the existing workspace/history name; name availability has not been established. | [Latest answers](launcher-design-interview.md#latest-answers-q36q41) |

## Runtime direction and evidence

The user reopened the Node/native choices to pursue WIT bindings plus a Wasmtime runtime, then explicitly required **WASI 0.3, not 0.2**. This is the current architecture under evaluation, with component-native async required by [ADR 0013](adr/0013-require-wasi-03.md). A mixed P2/P3 result does not meet that requirement. The shared helper, per-generation stores and public WIT/SDK design remain engineering proposals, not a completed runtime.

**QuickJS is provisional.** The user requested that its findings be recorded and further experiments deferred during grilling. The JS engine is replaceable in principle; no universal Node/npm compatibility, long-lived fork or final engine selection is accepted. WIT generates interfaces/bindings; it does not itself execute JS. TS was transpiled for the experiments.

| Saved evidence | What it establishes | What it does not establish |
| --- | --- | --- |
| [Rust std P3 probe](research/p3-std-spike/README.md) | A pinned nightly + SDK 34 build runs ordinary std filesystem/environment/clock checks with P3-only imports. | All Rust crates, Tokio, or every target working unchanged. |
| [Patched QuickJS P3 probe](research/qjs-p3-port-spike/README.md) | Fuse/Zod, native async, file streams, 20 sequential same-instance calls and error cases pass on Windows with P3-only imports. | Production readiness, concurrent-call isolation, cancellation, leak freedom or universal JS library compatibility. |
| [P3-only embedded host](research/p3-only-host/README.md) | Accepts the patched JS/Rust guests and rejects stock mixed-version JS. | Production helper IPC, crash supervision or launcher integration. |
| [GPUI probe](research/wasi03-gpui-spike/README.md) | Windows rendering of guest-produced JSON, file-based view replacement and click recording. | Actual guest hot reload, live Wasmtime/GPUI event round trips, custom drawing, IME/accessibility parity or macOS/Linux support. |

The QuickJS integration patch removes the old adapter/reset path, links real P3 libc, removes P2 registration and moves async task bookkeeping to separate TLS. Build flags export TLS metadata. These are componentize-qjs integration changes, not changes to the JS engine source. The snapshotted Math.random/performance state that repeated across fresh instances is fixed in a candidate patch with a regression check. On Linux, JS and TS samples of `wit/extension.wit` run through `pane-core`. [Linux validation](research/js-backend-validation/README.md). **Decided 2026-09-28 (#2):** build JS/TS extensions with pinned upstream componentize-qjs (`e563c6d6`) plus Pane's patch queue (P3 port and snapshot reseed), propose the patches upstream, and do not maintain a long-term fork. QuickJS remains replaceable if upstream diverges. [Patch and detailed report](research/qjs-p3-port-spike/README.md).

Recorded same-workload comparison: component 6.67 to 9.44 MiB; median fresh cached CLI peak process working set 30.71 to 36.81 MiB (three samples). Different optimizations mean this is not an isolated measure of WASI3 overhead, launcher idle memory, or additive per-extension cost. Keep the earlier smaller synchronous benchmarks separate. [Raw measurements](research/qjs-p3-port-spike/measurements.json).

## Cross-cutting details preserved

- Clipboard history is off until enabled, with configurable finite retention, pause/disable and deletion controls. Expiry continues while disabled. Exact default retention remains open. [Clipboard policy](extension-policy-proposal.md#clipboard-history).
- Disable preserves settings and unexpired data; uninstall removes managed code/cache/local credentials and offers a durable-data choice. User-owned source folders and external documents are not silently deleted. [Data policy](extension-policy-proposal.md#disable-cache-data-and-uninstall).
- Saved state survives reload; arbitrary transient state requires explicit extension support. Successful compilation followed by failed startup does not restore an older version automatically. Cleanup cannot reverse arbitrary external side effects. [Reload policy](extension-policy-proposal.md#development-reload).
- Automatic failure recovery is not the same operation as user-requested disable/uninstall with dependent handling. Expected operation errors should not pause a whole extension; shared-runtime crashes may lack a known culprit. These distinctions are proposed mechanics supporting Q39, not tested guarantees. [Recovery proposal](launcher-design-interview.md#proposed-automatic-recovery-behavior-for-q39).
- Source distribution flexibility does not imply end users compile arbitrary source-only packages without tools. Native integrations may need prebuilt helpers. [Setup policy](extension-policy-proposal.md#end-user-installation).

## Explicitly unresolved or deferred

These remain open design/validation work in the specification and implementation issues:

1. Final JS engine, upstream patch/fork strategy, runtime packaging/topology and initialization fixes.
2. Public WIT/SDK contracts, language parity, UI/event/custom drawing, input/IME/accessibility and host/helper IPC.
3. Cancellation, hangs, concurrent work, stale generation replies, reload/disable resource cleanup and fatal shared-runtime recovery.
4. Package manifest/artifact format, dependency/resource/operation addressing, dependency cycles/partial installation, Git tracking and staged update activation.
5. API version signaling, deprecation/migration details, persisted-state schema/migration ownership and recovery limits.
6. Supported OS versions/architectures/Linux desktops, release validation matrix and measured resource/startup/interaction targets.
7. Installer acquisition/retry/cache mechanics; app/runtime update delivery; default clipboard retention and other concrete settings defaults.
8. The licensing split is decided and applied; release notice generation (cargo-about plus a manual supplement) and the per-release Corresponding Source procedure remain to be exercised on a real artifact.
9. Catalog and historical-version picker are explicitly deferred; Python/C# are later languages. Neither is silently restored to launch scope.

The specification identifies bounded validation tasks and open contracts. A consequential new product choice still needs a user decision before implementation depends on it.

## Workflow checkpoint

The [specification](https://github.com/hoangvu12/pane/issues/1) and [52 implementation issues](https://github.com/hoangvu12/pane/issues?q=is%3Aissue+label%3Aimplementation) are published in GitHub Issues with `ready-for-agent` labels. Start with [#5](https://github.com/hoangvu12/pane/issues/5). [#6](https://github.com/hoangvu12/pane/issues/6) also requires the [JS/TS backend prerequisite #2](https://github.com/hoangvu12/pane/issues/2). Labels do not clear native blockers or approve unresolved testing preferences.

The [contributor requirement](https://github.com/hoangvu12/pane/issues/1#cross-platform-contributor-requirement) gates shared features on early native Windows/macOS/Linux workflows. [Release validation](https://github.com/hoangvu12/pane/issues/57) tracks platform-specific distribution and readiness evidence. Production implementation has not started.

Both AGENTS.md and CLAUDE.md are retained by user choice. Follow the [GitHub tracker conventions](agents/issue-tracker.md), [triage roles](agents/triage-labels.md), and [domain rules](agents/domain.md).

## Record limits

The saved interview contains Q1-Q41 and later corrections, but is not a verbatim conversation transcript. Earlier claimed pre-audit snapshots are absent; do not assume they can be recovered. Historical audit reports and removed draft material remain in Git history.

Research reports describe saved experiments, not newly rerun checks or production support guarantees. Some toolchains and built artifacts use temporary local paths; the saved source, patches, pinned dependencies and result files provide the reproducibility record. Keep unresolved decisions and platform limits explicit.
