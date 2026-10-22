# VS Code manual extension version selection (Q32)

Research date: 2026-09-27. Source audit pinned to `4336606aa949efdcddb89b3db3e216562730b2bf`; inspected source, not a running VS Code installation. Q32 remains a proposal for our launcher.

## Findings

VS Code supports selecting a specific available extension version through the extension context menu. Its current documentation calls this **Install Another Version**. [Official documentation](https://code.visualstudio.com/docs/configure/extensions/extension-marketplace#install-an-extension)

In the inspected source, the action's label is **Install Specific Version...**. It fetches compatible versions for the target platform, shows a picker, and passes the selected version into installation. Picking the already-installed version returns without reinstalling it. [Picker and install call](https://github.com/microsoft/vscode/blob/4336606aa949efdcddb89b3db3e216562730b2bf/src/vs/workbench/contrib/extensions/browser/extensionsActions.ts#L1576)

Selecting another version also pins that installation through this code path:

1. The workbench translates an explicit `version` into `installGivenVersion = true`. [Workbench installation](https://github.com/microsoft/vscode/blob/4336606aa949efdcddb89b3db3e216562730b2bf/src/vs/workbench/contrib/extensions/browser/extensionsWorkbenchService.ts#L2731)
2. Desktop extension installation writes `pinned: true` when `installGivenVersion` is set. [Stored metadata](https://github.com/microsoft/vscode/blob/4336606aa949efdcddb89b3db3e216562730b2bf/src/vs/platform/extensionManagement/node/extensionManagementService.ts#L1084)
3. With ordinary global auto-updates enabled, the update policy skips pinned extensions. Enabling auto-update for a pinned extension explicitly clears its pinned metadata. [Update policy](https://github.com/microsoft/vscode/blob/4336606aa949efdcddb89b3db3e216562730b2bf/src/vs/workbench/contrib/extensions/browser/extensionsWorkbenchService.ts#L2350), [clear pin](https://github.com/microsoft/vscode/blob/4336606aa949efdcddb89b3db3e216562730b2bf/src/vs/workbench/contrib/extensions/browser/extensionsWorkbenchService.ts#L2457)

Thus **manual version selection plus protection from ordinary automatic replacement has a concrete VS Code precedent**. It does not require a separate manual auto-update-off step in that normal path.

Qualification: this is not an unconditional forever-pin guarantee. The same policy checks `forceAutoUpdate` first; when global auto-update is off, explicit per-extension/publisher opt-ins are evaluated before the pinned check. The source supports describing normal behavior without claiming pins override every special policy. [Policy ordering](https://github.com/microsoft/vscode/blob/4336606aa949efdcddb89b3db3e216562730b2bf/src/vs/workbench/contrib/extensions/browser/extensionsWorkbenchService.ts#L2350)

## Relevance to our launcher

Supports the Q32 recommendation: allow explicit older-version selection where an artifact/revision is available, preserve that selection against ordinary automatic updates, and provide an explicit way to resume updates. This source audit establishes code version selection; it does not establish reversal of extension data migrations. No launcher behavior is accepted solely by this research.
