# Dependencies on other extensions

Research for Q26, 2026-09-27. The user requested precedent research before settling installation behavior. Q25 call-and-result support is accepted; Q26 automatic installation of required extensions was subsequently accepted; see [extension policies](../extension-policy-proposal.md). Source/documentation review only, with no installation or disable/uninstall experiments.

## Distinguish three mechanisms

An npm library dependency is code a package imports. A package can also bundle several extension entry points. A dependency on another installed launcher extension means the host must resolve that extension's identity, version, activation and lifecycle. These are different contracts; successfully installing an npm dependency does not alone establish a separately managed launcher extension.

## Raycast

Raycast's manifest extends npm's package format. The inspected reference lists extension commands, tools, preferences and platform fields, but no documented equivalent of VS Code's `extensionDependencies` field. This is a scoped documentation finding, not a claim about every internal feature or community helper. [Manifest](https://developers.raycast.com/information/manifest)

Its `launchCommand` API throws when a target command does not exist or is disabled. The documented call therefore does not automatically satisfy a missing extension dependency. Authors may build their own installation guidance around a failure; that is not an automatic dependency solver established by this API. [Command API](https://developers.raycast.com/api-reference/command)

## VS Code

VS Code declares required extension identities in `extensionDependencies` and installs missing required extensions recursively. `extensionPack` instead describes a collection installed together; its members are not necessarily runtime requirements. Dependencies are plain extension IDs, not npm-style per-dependency version ranges. See [the manifest reference](https://code.visualstudio.com/api/references/extension-manifest) and [the source audit](vscode-extension-dependencies.md).

The source audit also finds that enabling an extension can recursively enable installed dependencies and extension-pack members. Dependency removal/disable flows account for dependents. Therefore VS Code is useful precedent for automatic installation, but is not evidence for our proposal to preserve a deliberately disabled dependency in every flow. Exact behavior depends on the operation; see [enablement and removal paths](vscode-extension-dependencies.md).

## Pi

Pi installs npm library dependencies for managed npm/Git packages. Authors can bundle other Pi packages and explicitly expose their extension resources, including through declared `node_modules` paths. However, automatic discovery skips `node_modules`; installing a library dependency does not automatically activate it as another separately managed extension. [Pi source audit](pi-extension-dependencies.md)

The inspected Pi manifest/resource resolver has no dedicated required/optional dependency graph between separately installed extensions. Package-source pins and resource enablement are separate mechanisms. Our proposed relationship declarations and conflict handling would therefore add behavior beyond Pi's conventional package model. [Pi source audit](pi-extension-dependencies.md)

## Detailed primary-source audits

- [VS Code dependency installation, enablement and versions](vscode-extension-dependencies.md).
- [Pi package dependencies versus installed extension resources](pi-extension-dependencies.md).

## Accepted launcher direction and remaining implementation work

Use explicit declarations for dependencies on other launcher extensions. Required dependencies are listed in the install summary and compatible missing targets are installed with the requested package. Optional integrations are used when present, without automatic installation. A deliberately disabled dependency stays disabled, with a clear unavailable-dependency explanation for affected commands. This respects the previously accepted disable/call behavior.

Keep the initial model bounded: resolve one installed target per extension identity and reject incompatible requirements clearly instead of silently changing pinned versions or promising parallel dependency versions. Exact identity/source selection, version constraints and conflict resolution still need design; Q26 accepts the installation behavior, while those exact mechanisms remain open. npm libraries retain their language/package-manager semantics.

Dependency declarations describe a supported integration relationship under full trust, not a new permissions sandbox. Installation order, rollback after partial failure, dependency removal and transitions after an update adds a requirement need definition before implementation. Do not adopt other products' automatic enablement behavior if it contradicts the already accepted promise to leave disabled targets disabled.
