# Disabling or removing an extension dependency

Research for Q27, 2026-09-27. Q26 automatic installation of declared required extensions is accepted; Q27 subsequently selected the VS Code-style disable/uninstall-together normal flow with "prob do vscode"; see [extension policies](../extension-policy-proposal.md). Source inspection and documentation review only, without running product uninstall flows.

## VS Code

The normal Extensions UI checks which installed extensions depend on the target. Disabling a required dependency offers a dependent-disable operation or cancellation; uninstalling offers removal of dependents together or cancellation. The underlying uninstall service also checks dependent relationships, with explicit bypass options for other callers. This is a different policy from leaving dependents installed but temporarily unavailable. See the [Q26 audit](vscode-extension-dependencies.md) and [Q27 recovery/state audit](vscode-dependency-removal.md).

Do not equate user-disabled state with availability computed from missing dependencies. Whether restoring a provider revives a consumer depends on which state the prior operation changed. The targeted audit checks those paths rather than inferring recovery from the existence of an enable button.

For an ordinary A-requires-B relationship, the UI's Disable All operation stores both A and B as explicitly disabled. Re-enabling B alone does not re-enable reverse-dependent A. Uninstall All removes both; reinstalling B does not establish an automatic reinstall of A. VS Code also has a separate computed dependency-disabled state, but it is not the state written by this UI cascade. These are source-derived findings for the normal flow, excluding independent extension-pack relationships and force-removal paths. [State and recovery evidence](vscode-dependency-removal.md)

## Pi

Conventional Pi does not model a declared dependency graph between separately managed extensions in the inspected manifest/resource resolver. Resource selection filters which extensions load, so there is no equivalent built-in cascade of dependent-disable states in that path. Bundled resources and ordinary npm imports are different relationships. [Pi dependency audit](pi-extension-dependencies.md)

The package manager's `removeAndPersist` removes the requested package and its configured source. npm removal delegates to its package manager; Git removal uses its package path; local source handling leaves the user's source directory intact. That method does not implement a cross-extension dependent-removal graph. These are scoped source findings and do not describe npm's own library graph. [Removal dispatch](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/package-manager.ts#L1035), [npm uninstall](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/package-manager.ts#L1844)

Pi documents `pi remove` for configured packages and `pi config` for resource enablement. A cooperating extension's behavior when another provider disappears depends on its own integration code; the reviewed host model does not establish our proposed missing-dependency badge or automatic recovery contract. [Package commands and resource selection](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/docs/packages.md)

## Raycast

The documented cross-extension `launchCommand` rejects missing or disabled targets. The calling extension can handle the failure. This supports a clear error at call time, but does not establish automatic dependent removal, suspension, or restoration. The scoped [manifest/dependency comparison](extension-dependencies.md) found no corresponding declaration of a separately installed extension dependency graph. [Command API](https://developers.raycast.com/api-reference/command)

## Earlier recommendation, superseded by the user

The user chose the VS Code-style normal flow: present required dependents and offer a combined disable/uninstall or cancellation. Restoring the dependency alone does not restore dependents; existing launcher data-retention/deletion policies still apply. The retained-unavailable proposal below is historical, not the current policy.

Show the affected required dependents before applying a user's disable/removal. Allow that action while retaining dependent packages and their settings/data; make them unavailable with a concrete missing/disabled/incompatible-dependency reason. Stop affected managed execution under the accepted lifecycle contract. Never silently uninstall dependents or reinstall/re-enable the removed dependency.

Keep the user's enable/disable preference separate from computed availability. When the dependency returns compatibly, a dependent the user still wants enabled can become available again under normal lazy activation; a separately user-disabled dependent stays disabled. This is our proposed policy, not an exact copy of VS Code or Pi. Optional integrations lose only the functionality that needs the provider. Dependencies required by the entire extension block its activation; per-command dependency declarations would be a separate design extension.

Exact transitive checks, in-flight call failures and recovery after a partial operation remain implementation work. A cached package or automatic update must not bypass an explicit removal by treating the missing dependency as permission to reinstall it on ordinary activation.
