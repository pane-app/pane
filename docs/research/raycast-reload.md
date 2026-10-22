# Raycast development reload and state

Checked 2026-09-27 against official documentation. No Raycast runtime experiment was performed. The user subsequently accepted the combined reload recommendation in [ADR 0004](../adr/0004-reload-extensions-without-restarting-launcher.md); proposed internal implementation details below remain open.

## Trigger and scope

`npx ray develop` enables development mode. Saving changes automatically reloads commands unless the developer disables auto-reload. Logs appear in the terminal, and errors include UI diagnostics. On a build failure, the documentation explicitly says the last successful command keeps running. This supports rebuilding before replacing working code; it is not a documented guarantee of rollback after new code starts and performs side effects. The CLI page describes command reload without restarting the Raycast application. It does not precisely document worker/process replacement or the dependency invalidation graph. [CLI](https://developers.raycast.com/information/developer-tools/cli)

## What survives

Raycast's normal command lifecycle is launch, execution and unload. On unload, the entire command is removed from memory. That lifecycle is evidence of command-scoped execution, not by itself a specification of every development-reload detail. [Lifecycle](https://developers.raycast.com/information/lifecycle)

There is direct evidence that reload can reset live state: the OAuth documentation tells developers to avoid auto-reloading during authorization because the OAuth client is reinitialized, causing a state mismatch. Do not equate Raycast's automatic reload with transparent continuation of requests or React Fast Refresh that preserves arbitrary state. The reviewed docs do not guarantee preservation of all React hook state, navigation or selection. [OAuth flow](https://developers.raycast.com/api-reference/oauth)

Authors can explicitly persist values using LocalStorage, and `useCachedState` keeps JSON-serializable values between command runs. Persistence is separate from preserving an existing component instance, closure or native handle. [Storage](https://developers.raycast.com/api-reference/storage), [useCachedState](https://developers.raycast.com/utilities/react-hooks/usecachedstate)

## Launcher implication: recommendation only

Borrow automatic rebuild on save, a toggle/manual reload option, useful build diagnostics and preservation of working code when compilation fails. Keep the GPUI CE application open. Recreate the affected extension's execution after a successful build, preserve durable data, and support optional explicit state export/restore. Rust needs compilation before replacement; the same lifecycle can apply to both language SDKs, but compile time will differ.

Host-owned query/navigation state can survive where identifiers remain compatible. Cancellation and cleanup should retire old callbacks, tasks and view resources; new views receive a fresh extension generation. This is our proposed contract, not Raycast's documented internal implementation. Runtime-start failures and migration failures need distinct handling from compiler errors; do not promise universal rollback.
