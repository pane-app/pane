# Extension policies

**Current-status source:** [reconciled decision index](current-decisions.md), audited 2026-09-28. Historical Node/native entry-point decisions are superseded as the runtime baseline and preserved in the interview and ADRs. These policies are intended behavior, not implemented guarantees.

**Q39/Q40 follow-up:** User selects Zed-style licensing provisionally; exact SDK/component licensing remains open and no license file is applied. User selects automatic recovery through the UI: skip an identified failing extension and notify them, without requiring a CLI command. Detailed failure handling is proposed in the [interview](launcher-design-interview.md#proposed-automatic-recovery-behavior-for-q39): distinguish expected operation errors, attributable extension failures and unattributed shared-runtime crashes; preserve data, show persistent status and Retry/Details, suppress crash loops, and avoid replaying side-effecting actions automatically. These are design requirements/recommendations, not validated implementation.

**Latest Pane product decisions (Q36–Q41, 2026-09-28):** Pane is the accepted product name. User prefers internet-first acquisition of runtime/default-feature payloads rather than including them in the installer; precise bootstrap/download design remains proposed. Normal users still need no manual runtime/compiler setup. Application updates notify the user and are installed at their choice; do not infer automatic application downloads or restarts. This is separate from automatic compatible extension updates below. Only Windows prototypes have been tested; macOS/Linux remain intended targets and previews may be staggered. Recovery mode details and licensing remain under discussion. See [latest answers](launcher-design-interview.md#latest-answers-q36q41).

**Required WASI version: 0.3**, explicitly selected by the user. See [ADR 0013](adr/0013-require-wasi-03.md). Guest toolchain/API validation remains open; p2 tests do not establish compliance.

**Runtime direction reopened after Q32:** the user wants to evaluate wit-bindgen + Wasmtime as the primary route and believed it was already selected. Prior Node/native execution clauses below record earlier decisions under review. [The new evaluation](research/wasm-primary-evaluation.md) demonstrates Rust and TS guests executing in Wasmtime; GPUI, full-trust capability priority, lifecycle/source/update policies remain. Do not present the old runtime choices as immutable or the candidate SDK as implemented.

The user chose Pi-style trusted extensions/distribution for Q9-Q10 and the disable/data behavior for Q11. See [ADR 0002](adr/0002-trusted-extensions-and-open-distribution.md). Later decisions require WASI3 and evaluate WIT/Wasmtime as the primary runtime route; QuickJS remains provisional. The filename is retained for stable links.

## Trust and distribution

Extensions are trusted local code. Prioritize capability: filesystem access, networking, subprocesses, native tools and OS integrations are allowed by the product model, subject to OS permissions and runtime support. Do not build a mandatory per-extension permission sandbox or permission-expansion approval flow into the initial design.

Use our own extension API with Pi-style npm, Git and local-path distribution. Q35 defers catalog/discovery-product decisions until the launcher and extension system work; a future catalog is optional and is not a required installation gate. Pi-style distribution does not promise unchanged compatibility with Pi or Raycast extensions. Exact packaging, version selection and update UX remain implementation decisions.

Package installation and any build hooks belong to the trusted-code model; built-artifact-only installation is no longer a requirement. Resolve supported package-manager behavior during implementation rather than assuming every source must run a build script.

WIT/Wasmtime is the current architecture under validation, with WASI 0.3 explicitly required. Earlier Node/native extension entry-point choices are superseded as the implementation baseline under evaluation. Q33 accepts optional prebuilt native helpers invoked through the SDK/host API, with managed execution and cleanup, for functionality unavailable inside the guest. Supported packages need no normal-user compiler setup. Exact helper packaging remains open; no universal library compatibility is promised. See [ADR 0014](adr/0014-optional-native-extension-helpers.md).

Retain lifecycle ownership and cleanup for reliability. Host-managed tasks, callbacks and child processes must follow disable/reload behavior. Trusted extensions are responsible for cooperating when they bypass host-managed APIs; the app cannot promise containment of arbitrary detached programs or reversal of external side effects.

## Launch authoring languages

JavaScript, TypeScript and Rust extension authoring are required at launch; Python and C# can follow. The current target is WASI 0.3 components in a Wasmtime helper, with a language-neutral view/event contract for GPUI CE and optional native helpers under Q33. Earlier Q18/Q19/Q22/Q23 Node/native entry-point decisions are historical and under review, not the current implementation baseline. The [executable probes](research/wasi03-validation.md) now prove P3-only Rust std and a patched JS/TS QuickJS path, including native async and filesystem streams in a P3-only host. Stock QuickJS remains mixed-version; the temporary port still needs runtime initialization/lifecycle work and size optimization. These are feasibility results, not a finalized SDK.

## Platform-specific extensions and actions

Q34 accepts extensions that support a subset of Windows, macOS and Linux, and extensions with individual platform-specific actions. Keep this simple: supported-OS metadata and straightforward action availability checks with a clear explanation when unavailable. Preserve the usable portion of an extension. Our core launcher still targets all three platforms. Exact manifest/API syntax and OS baselines remain open; no general compatibility-rule framework is required. Q33 native helper artifacts must match their supported OS/architecture.

## Main search

Accepted in Q17: use Raycast-style search behavior. Root search shows applications, extension commands, quicklinks, calculator answers and enabled file results. Service-content search happens inside the selected integration command; aliases, hotkeys and fallback actions provide faster access. Automatic searching across every online integration is deferred from the initial version.

The core owns the input, command registry, matching/ranking, aggregation, navigation and action dispatch. Extensions supply feature-specific data/results, including the disableable default app, calculator, file and quicklink features. Metadata can be indexed without starting every extension. Additional global-provider modes can be considered later; their launch API is not committed. See [ADR 0006](adr/0006-raycast-style-search-with-extension-providers.md).

## Activation and background work

Accepted in Q15: load executable extension code on demand for commands/events, and support scheduled work plus explicit continuous background services. Keep lightweight contribution metadata available without starting every extension. Installing an extension does not require a permanently active instance. Background services run when enabled; disabling stops their host-managed work. The model applies equally to bundled and external extensions and does not restrict trusted code's functional capabilities.

Activation policy does not select the final Wasmtime helper topology, idle-unload timeout or background scheduling interval. Those remain implementation details to measure. Clipboard monitoring requires ongoing event observation while enabled; a scheduled refresh has a different lifetime. Historical Q23 Node workers are not the current baseline.

## End-user installation

Accepted in Q16, with the user's additional preference that users should not need to install anything else to use the launcher. Ordinary users install the launcher and supported extension packages; the app bundles or automatically manages required runtime/installation dependencies. No manual Node, Rust, npm, Git or compiler setup should be necessary for that normal path. See [ADR 0005](adr/0005-lazy-activation-and-managed-dependencies.md).

Support ready-to-run JS/TS and Rust component packages under the WASI3 direction, plus prebuilt optional native helpers for supported OS/architecture targets. Preserve npm/Git/local source flexibility. Developer source builds can need toolchains; an arbitrary source-only package is not automatically ready to run. Explain unsupported artifacts/dependencies. This is a packaging responsibility, not a ban on trusted build hooks.

Q36 selects the internet-first preference: omit runtime/default-feature payloads from the installer and assume an initial connection. Automatic acquisition/cache is the proposed mechanism; download timing, progress/retry UX and packaging remain open. Offline first use is not promised. Q22's Node-specific mechanism is historical. External accounts/applications remain requirements of their integrations. Normal runtime management should not modify the user's global developer environment.

## Extension-to-extension calls

Accepted in Q25: provide host-routed calls to explicitly published programmatic operations, with structured inputs and asynchronous results, completion or errors. Support callers and targets written in JS/TS or Rust. Authors choose which operations are callable; launching an ordinary UI command does not imply a result-returning API. See [ADR 0011](adr/0011-extension-call-and-result-api.md).

Resolve the target and report missing, disabled or incompatible operations without silently enabling an extension. Concrete SDK syntax, serialization/schema rules, cancellation and recursive calls remain design work. Q26 selects required/optional installation behavior below. Full trust remains unchanged; this does not adopt a visual workflow builder or an elaborate dependency system.

## Dependencies on other extensions

Accepted in Q26 after the [comparison](research/extension-dependencies.md): authors declare required and optional dependencies on other launcher extensions. List required dependencies in the install summary and automatically install compatible missing ones with the requested extension. Optional integrations are not automatically installed. Preserve deliberately disabled dependencies and explain unavailable functionality; report version/platform conflicts without silently replacing pinned versions.

This is distinct from ordinary npm library dependencies and does not commit to an elaborate multiple-version dependency solver. Q29 selects source-derived package identity below; detailed dependency/resource addressing, compatibility metadata and partial-install failure handling remain open. Q27 selects the disable/removal behavior below.

Accepted direction in Q27: use the VS Code-style normal UI for required dependents. Show affected extensions and offer Disable All or Cancel when disabling a required dependency, and Uninstall All or Cancel when removing it. Apply the confirmed operation to the required dependent set. Disabling preserves saved data; uninstalling follows the existing managed-data policy and explicit durable-data choice for the affected set. Optional integrations do not cause dependent removal.

Re-enabling the dependency alone does not automatically re-enable dependents disabled by this operation; reinstalling it alone does not reinstall removed dependents. This replaces the earlier proposed normal flow that retained dependents as temporarily unavailable. It does not change Q26 or Q25: installing/calling another extension does not silently re-enable a deliberately disabled dependency. Failure states from missing/incompatible packages outside this normal flow still need clear diagnostics.

## Conflicting installations

Accepted in Q29: use Pi-style source-derived package identity. npm identity is the package name without its version; Git identity is normalized host/repository path without its ref; local identity is the resolved absolute path. Titles are separate. Developer-defined universal publisher/name IDs and generated permanent UUIDs were considered and not adopted. See [ADR 0012](adr/0012-pi-style-source-identity.md).

Apply accepted Q28 duplicate-install rejection to the canonical source identity: a second explicit install of that identity reports already installed; normal updates remain supported. Equivalent Git SSH/HTTPS references identify the same repository. Different versions/refs do not become separate package identities. npm, Git and local copies can have distinct identities even when their code is the same. Do not automatically merge their settings, switch sources or repair conflicts; users/authors remain responsible. Report dependency/version conflicts clearly.

This adopts Pi's identity model, not every Pi precedence rule: launcher personal/project scopes are not selected. Source moves/renames can change identity; automatic data migration is not promised. Q30 selects manual development-copy control below. Package identity must be distinguished from extension-resource and operation addressing when implementing dependencies and cross-extension calls. Exact multi-resource packaging and SDK addressing remain open.

## Development and published copies

Accepted in Q30: developers manage local development copies and published installations themselves. Leave distinct source identities independently enabled/disabled. Starting or stopping development does not automatically replace, disable, restore or remap a published installation. Developers use the normal disable/uninstall controls when they want only one copy active. Q14 save/build/reload still applies to the affected development extension.

## API evolution and maintenance

Accepted in Q24: minimize unnecessary API changes and favor stable interfaces, while retaining the ability to make breaking changes when the launcher needs them. Extension authors maintain and adapt their packages; the launcher is not required to keep obsolete interfaces indefinitely or take over abandoned extensions. This applies to JS/TS and Rust. See [ADR 0010](adr/0010-best-effort-extension-api-compatibility.md).

The earlier strict major-version compatibility promise was not adopted. Exact version signaling, deprecation timelines and migration tooling remain open. Continue to check declared compatibility as required by the update policy and explain known incompatibility clearly; stability is an engineering preference, not a guarantee that every old extension runs forever.

## Extension updates

Accepted in Q21: automatically update compatible, unpinned published extension packages, with global and per-extension opt-out controls plus manual Update/Update all. Respect explicit version pins and do not auto-overwrite local development folders. Avoid replacing an extension midway through an active command. Git tracking/ref policy, exact activation timing for persistent views/background services, and implementation details remain open.

Check declared host/API/platform compatibility before activation. Retaining previous code artifacts is recommended where practical, but code reversion does not reverse data migrations. Q38 separately selects application-update notifications with installation at the user's choice; automatic app downloads/restarts are not accepted. Managed-runtime update details remain open. See [precedent research](research/extension-failures-updates-offline.md).

## Extension UI and renderer

Use GPUI CE with Pi-style standard controls plus custom interactive extension views (Q13). See [ADR 0003](adr/0003-gpui-ce-and-extensible-views.md). The shared UI contract, custom drawing and communication protocol need design and validation for JS/TS and Rust under the WASI3 direction. The Windows JSON-rendering probe is not a live guest/UI bridge.

## Extension failures

Q20 accepts keeping Pane usable, Retry/View logs, preservation of saved data and suppression of repeatedly failing background activation. Q39 adds automatic detection/skipping of an identified broken extension with a UI notification, without requiring CLI recovery. This is a reliability objective under full trust, not containment of arbitrary code.

Proposed mechanics distinguish operation errors from fatal/repeated extension failures, keep persistent failure status with Retry/Details, and avoid replaying actions with possible completed side effects. A shared runtime crash can interrupt several extensions without identifying a single culprit. Process boundaries, attribution, thresholds and persistent recovery remain design/validation work. See [the recovery proposal](launcher-design-interview.md#proposed-automatic-recovery-behavior-for-q39) and [historical comparisons](research/extension-failures-updates-offline.md).

## Development reload

Accepted in Q14: saving development code triggers rebuild and reload of the affected extension, with a manual reload option. Keep the GPUI CE launcher open. Build/compilation errors preserve the working extension and show diagnostics; after a successful build, clean up the old instance and start the replacement. Apply the same lifecycle to JS/TS and Rust, with the appropriate build step for each.

Preserve saved data. Temporary state restoration requires explicit extension support; live objects and running tasks need not survive replacement. Retaining compatible host-owned search/selection state remains an implementation recommendation, not a guarantee for arbitrary custom UI. Q31 settles replacement startup failure below; detailed cleanup and migration recovery mechanics remain open. See [ADR 0004](adr/0004-reload-extensions-without-restarting-launcher.md).

Accepted in Q31 after [precedent research](research/extension-startup-failure.md): when replacement code builds successfully but fails during startup, clean up its host-managed work, report the instance as failed, and offer Retry and logs. Do not automatically restore an older version. The normal save/build/reload cycle remains available after a developer fixes the code. Keep the Q20 suppression of repeatedly failing background activation. Build failure before replacement still preserves the working instance under Q14.

Preserve host-managed saved data; startup recovery does not automatically delete it or reverse migrations. Trusted extension code may already have modified data or external systems. This policy concerns replacement startup failure, not an installation transaction failing before activation or the separate recovery of a crashed shared Node process. Detailed cleanup and runtime restart mechanics remain open. Accepted in Q32: defer the historical-version/downgrade picker for the initial release. Existing pin/update controls and explicit supported source version/commit specifications remain; exact source-input UX is implementation work, without a promise that every historical artifact is available.

## Disable, cache, data and uninstall

| Operation | Accepted behavior |
|---|---|
| Disable | Stop activation, in-flight host-managed work, timers, subscriptions and contributed commands/hotkeys. Persist disabled state. Keep saved settings/data. |
| Re-enable | Reuse retained settings/data after expiry cleanup and any compatible migrations. |
| Clear cache | Remove disposable downloaded/derived data; preserve settings, user content and credentials. |
| Delete history/data | Delete the selected app-managed records with a clear scope. Pause writes while deleting; explain whether collection continues afterward. |
| Uninstall | Remove installed extension code, compiled caches and locally stored credentials. Show what durable data will remain, with an explicit delete-data choice. Local development source folders remain user-owned. |
| Remove retained data later | Allow deletion from host settings even when the extension is disabled or absent. |

Disabling retains local credentials for convenience and stops the component from executing through the host. Uninstalling removes host-managed local credential copies. This is lifecycle behavior, not secret isolation from trusted code. Remote OAuth/session revocation is separate and provider-specific; do not promise it merely because local tokens were deleted.

Uninstall must not silently delete documents a user created in ordinary external folders, unrelated downloads, remote service content or the operating system's current clipboard. Label the scope as the launcher's managed data; an unrestricted external program may have written elsewhere. Deletion cannot promise forensic erasure from SSDs, backups or remote services.

Cleanup should be controlled by the host, not depend on running a broken or removed extension. Extension-owned records need identity, type (settings/content/cache/secret), retention metadata and predictable storage ownership. Persistence and runtime instance lifetime are separate.

## Clipboard history

Ship available but off until enabled. Provide a full disable toggle, a temporary pause control, per-item/bulk deletion and a finite configurable retention period. Exact retention duration is still open. Respect sensitive clipboard markers and app exclusions when the platform exposes them; do not claim every secret can be detected.

Disable stops observation immediately and preserves unexpired history. Retention deadlines continue to apply while disabled: a generic host cleanup mechanism can expire records without executing the extension; enforce expiry before displaying data after downtime. Retention does not restart on re-enable.

Offer a combined **Disable and delete history** action for users who want both. A **Clear history** action does not itself disable future collection. The launcher does not automatically send clipboard history to AI or network integrations; this default is not a security boundary against trusted extensions.

## Verification needed before shipping

With the extension disabled, no host-managed clipboard capture, scheduled queries or new host operations may occur; stale async replies must be discarded. Restart must preserve disabled status. Re-enable must retain settings without reviving expired history. Verify cleanup of supported tasks and child processes, and data deletion after failed cleanup and extension crashes. Test data ownership with two extensions and a reinstall from a different source using the same display name; ownership is an organizational contract under full trust.
