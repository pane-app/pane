# Pi: extension failures, updates, and offline packaging

Research date: 2026-09-27. Read-only audit of conventional Pi coding-agent source pinned to `2b0a123de98318c2ff8069661721ce0c3794c34e`; no runtime or failure-injection tests. This concerns Q20–Q22. It does not change accepted launcher decisions.

## Q20: recoverable errors differ from process crashes

Pi catches an extension's import/factory failure, records its path and error, skips that extension, and continues loading others. Extension command handlers also have a catch boundary that emits an extension error instead of propagating the exception. The interactive UI displays extension errors and supplied stack traces. Sources: [loader](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/extensions/loader.ts#L558), [command execution](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/agent-session.ts#L1754), [error presentation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/modes/interactive/interactive-mode.ts#L2935).

The documented rule is to report handler failures and continue where possible. A failing `tool_call` handler blocks the tool; a tool execution failure becomes an error result. Authors release resources in `session_shutdown` and make cleanup idempotent. This is lifecycle cooperation, not automatic reversal of arbitrary extension side effects. [Extension errors and cleanup](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/docs/extensions.md#error-handling).

Pi runs these extensions in its own process. Its last-resort `uncaughtCrash` handler explicitly cites an exception in an extension's asynchronous child-process callback as an example: it restores the terminal, logs the failure, records crash information when possible, and exits with status 1. Therefore Pi does **not** establish the promise that every extension crash leaves the application alive. No per-extension restart budget or repeated-failure suspension policy was found in the scoped loader/runner/interactive paths. [Uncaught crash handling](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/modes/interactive/interactive-mode.ts#L4190).

Implication for our launcher: preserve Pi's capability and author cleanup contract, but use the accepted external runtime/helper boundary to keep a failed extension runtime from taking the GPUI host down in ordinary crash cases. Retry, logs, and pausing repeated background failures are our proposed product behavior, not behavior copied exactly from Pi. If multiple JS extensions share one process, a process crash affects that whole group; process layout remains a separate design choice.

## Q21: automatic checking, explicit updates

Pi starts a package-update check asynchronously during interactive startup. Available updates produce a notification directing the user to run `pi update --extensions`; failed checks are swallowed, and `PI_OFFLINE` skips them. The observed behavior is update notification rather than automatic replacement of ordinary installed packages. [Startup check](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/modes/interactive/interactive-mode.ts#L1111), [check wrapper](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/modes/interactive/interactive-mode.ts#L1210), [notification](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/modes/interactive/interactive-mode.ts#L4507).

The CLI distinguishes these actions:

| Command | Effect |
|---|---|
| `pi update` | Update Pi itself |
| `pi update --extensions` | Update all configured packages |
| `pi update <source>` | Update one configured package |
| `pi update --all` | Update Pi and packages |

Source: [CLI update reference](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/docs/cli.md#update-pi-or-packages).

Exact npm versions are pinned and excluded from package updates. Git updates reconcile the configured ref; they do not choose a newer tag or commit instead. Version ranges and mutable Git branches should not be described as immutable pins. Local packages remain local files. Sources: [package source documentation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/docs/packages.md#choose-a-source), [update filtering](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/package-manager.ts#L1092).

There are important exceptions to a blanket “never changes packages on startup” claim: resolution can install missing packages or reconcile an npm installation that does not match its configured version. Temporary unpinned Git packages can refresh during resolution. Offline mode suppresses those fetch/install paths. [Package resolution](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/package-manager.ts#L1251).

No general previous-version rollback or extension-data migration reversal guarantee was established by this audit. Pi supports the earlier manual-update candidate. The combined [comparison](extension-failures-updates-offline.md) revises the launcher recommendation toward automatic updates with controls for a general-purpose desktop audience. The user subsequently accepted that combined Q21 recommendation; Pi comparison facts are unchanged.

## Q22: embedded runtime does not imply every extension installs offline

Pi's inspected standalone build runs `bun build --compile` for Windows/macOS/Linux targets and packages themes, assets, documentation, examples, and platform helpers. That distribution embeds its runtime. The npm distribution is a different installation path requiring Node. This supports shipping a usable runtime with an application, but is not evidence of arbitrary third-party extensions or model weights included in the release. [Standalone build](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/scripts/build-binaries.sh#L108), [CLI README](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/README.md).

Pi's package installer defaults to an external `npm` command; npm/pnpm/Bun command configuration is supported. Git installation calls external `git clone` and may invoke the package manager for dependencies. Local package dependency setup remains the author's responsibility. Runtime embedding therefore does not guarantee toolchain-free or network-free extension installation. [Package-manager command](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/package-manager.ts#L1751), [Git installation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/package-manager.ts#L1861), [dependency guidance](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/docs/packages.md#declare-dependencies).

Earlier recommendation, subsequently superseded by the user choosing automatic managed Node download in Q22: bundle managed Node and the default local extensions so the first local use works offline. Download additional extension artifacts when installed. This is compatible with a small permanent core: installer contents and always-running memory are different measurements. No installer-size or memory comparison was performed here.
