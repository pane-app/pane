# Raycast and Pi: extension authoring and distribution

Checked 2026-09-27. This follow-up answers what “install extensions from somewhere else” means in each ecosystem. No extension was installed or executed.

## Raycast

Authors use TypeScript/React and the Raycast API. The app offers templates and source import; ordinary users can browse the Store. [Getting started](https://developers.raycast.com/basics/getting-started)

Development uses `ray develop`: saving triggers command reload, diagnostics surface in the command UI and terminal, and the documented build-error behavior retains the last successful command. The CLI can also produce a distributable `.rayext` archive through `ray bundle`. These facilities do not mean arbitrary desktop-app code is compatible with Raycast. [CLI](https://developers.raycast.com/information/developer-tools/cli)

The public Store publishing path submits a GitHub pull request. Acceptance and merge precede Store publication. Source import is a development path separate from reviewed Store distribution. [Publishing](https://developers.raycast.com/basics/publish-an-extension)

## Pi

A small extension can be one TypeScript file exporting a factory that receives Pi's API. It registers commands, tools, hooks or UI contributions; local TypeScript can load without a separate compilation step. Reload replaces the extension runtime and authors must clean up long-lived resources. [Extensions](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md)

Packages can come from npm, Git, or local paths. Local packages load without copying; explicit npm versions and Git refs can be pinned. One package can contain executable extensions and other resources. The package gallery is a discovery surface; the install mechanism accepts external sources. [Packages](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/packages.md)

## Implication for this launcher

Both ecosystems define their own host API. Where a package is downloaded from does not establish compatibility with a different application's API. A GitHub-hosted Raycast extension still expects Raycast services and UI semantics; a GitHub-hosted Pi extension expects Pi agent/session semantics.

Recommended combination: a coherent launcher UI and approachable authoring tools, with Pi-style external distribution. Users install through a simple graphical catalog or import flow; authors can work locally and publish elsewhere. Adopt one small launcher SDK first. Raycast compatibility can be investigated separately after the contract is established; Pi's extension API is a reference rather than a desktop UI API to implement wholesale.

## Flexibility without promising a frozen core

The user's flexibility preference should guide boundaries, not become a promise that core code never changes:

- A versioned SDK and protocol keep extensions from importing private application internals.
- Host-rendered view descriptions separate extension logic from the chosen renderer.
- OS adapters centralize platform differences.
- Owned registrations, cancellation and disposal make disable/reload behavior explicit.
- Installation sources feed a common validated package representation rather than separate runtimes.
- Execution mode is explicit. If v1 permits arbitrary Node APIs, adding a restricted mode later does not retroactively constrain those extensions while preserving all behavior.

These are candidate engineering choices, not accepted implementation decisions. Every additional API is a maintenance commitment; avoid designing a speculative plugin framework for capabilities no current workflow needs.

## Proposed disable contract

Disable removes contributed commands, stops extension-owned background work, unregisters shortcuts/listeners and prevents activation at the next startup. Persistent settings/data remain until a separate delete-data operation. Clipboard history stops collecting immediately when disabled. This proposal describes host-managed and cooperating extension resources; unrestricted third-party code may have launched independent processes or performed external actions that the host cannot reverse. Isolation and shutdown guarantees must be tested against the selected runtime.
