# Manual extension version selection and pinning

**Q32 status update:** the user challenged the historical-version picker as unreliable. The recommendation below is historical and superseded: now recommend deferring automatic version-history browsing for launch, retaining accepted pins, and allowing explicit exact-version/commit source input where supported. The user subsequently accepted deferring the picker: "ye right now its not needed". Existing pins/source controls remain; no dedicated historical-version UI is committed. Source discovery facts remain useful; advertised versions are not a guarantee of installability or reversible data changes.

Q32 research, 2026-09-27. User asks whether other projects offer manual recovery to an older extension version and protection from automatic updates. Q32 remains proposed; Q21 update controls/pin respect and Q31 no automatic startup rollback are already accepted. Read-only source/documentation research, no installation or downgrade experiment.

## VS Code: version picker plus automatic pinning

The official manual exposes **Install Another Version**. The inspected desktop source fetches compatible versions, passes the selected version to installation, sets `installGivenVersion`, and writes pinned metadata. Ordinary global automatic updates skip pinned extensions; explicitly enabling auto-update clears the pin. Selecting the already-installed version is a no-op. Therefore selecting another specific version normally holds it without an additional manual auto-update-off step. [Official picker documentation](https://code.visualstudio.com/docs/configure/extensions/extension-marketplace#install-an-extension), [pinned metadata](https://github.com/microsoft/vscode/blob/4336606aa949efdcddb89b3db3e216562730b2bf/src/vs/platform/extensionManagement/node/extensionManagementService.ts#L1084)

Special update-policy overrides exist, so do not promise pinning overrides every possible policy. See the [full version-selection audit](vscode-version-selection.md) for the picker-to-install-to-update-policy chain. The launcher has not adopted those special overrides.

## Pi: explicit version/ref in the source

Pi documents installation sources such as `npm:@example/pi-tools@1.0.0` and `git:github.com/example/pi-tools@v1`. A user can select an available older release through a source specification. This is a CLI/configuration model, not evidence of an in-app historical-version picker. Git sources can target a configured tag/commit; local sources remain local files. [Pi package docs](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/docs/packages.md#choose-a-source)

The source makes the npm distinction precise: only exact semver versions set `pinned`; ranges are not immutable version pins. Package updating excludes pinned npm targets. Git updates reconcile the configured ref rather than choosing a different release ref, so a branch remains a moving target and should not be presented as an immutable revision pin. [Source parsing](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/package-manager.ts#L1450), [update filtering](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/package-manager.ts#L1092)

## Raycast: latest Store version and changelog

Raycast describes one implicit latest Store extension version and automatic end-user updates; extension authors do not declare their own extension version field. Its Version History displays changes supplied by the extension changelog. This is not a documented old-version installation selector. [Versioning](https://developers.raycast.com/information/versioning)

The current user manual likewise describes automatic Store updates, a manual check, and View Version History to see what changed. Imported local development extensions are separately managed and do not receive Store updates. No standard Store downgrade/pin control was established by the reviewed official material. Do not claim no workaround exists or confuse checking out old extension source for local development with an end-user Store recovery feature. [Extension management](https://manual.raycast.com/extensions#check-for-updates)

## Proposed launcher behavior

Retain the Q32 recommendation: provide manual specific-version/revision selection for supported published sources and pin the selected version against automatic updates. Use a readable available-version picker where the source exposes suitable releases and an explicit Git revision input where appropriate; detailed UI is not settled. Pinning a fixed Git revision needs an immutable commit identity, not just a moving branch name. Resume normal automatic updates only after an explicit change to the pin/update choice.

This changes code within the tracked source identity; it is not a second same-ID installation and should not be rejected as a duplicate under Q28/Q29. Retain compatibility/dependency checks and the ready-to-run artifact requirement for ordinary users, including native Rust targets; an old source revision alone is not proof an installable artifact exists. Local development folders remain user-managed. Restoring code does not automatically restore prior data formats or reverse external side effects. The feature is proposed, not implemented or accepted.

## Follow-up: where the version list comes from

The user asked how versions would be discovered. See [version discovery](extension-version-discovery.md): npm registry metadata, supported host release APIs or generic Git tag advertisement, and no universal history for local folders. A published version/tag is a candidate; ready-to-run artifacts and declared launcher/platform compatibility must still be checked. This bounds the proposal rather than promising a universal old-version picker. Q32 remains pending.
