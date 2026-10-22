# Extension trust and distribution: comparison

Checked 2026-09-27 against official docs. No third-party extension was installed or run. User questions: Q9 asks for a recommendation grounded in existing products; Q10 asks whether an own-SDK ecosystem with external installation is how Raycast and Pi work.

Decision update: the user subsequently chose Pi-style full trust and distribution, explicitly prioritizing extension capability over hardening. The capability-based recommendation below is historical and superseded by [ADR 0002](../adr/0002-trusted-extensions-and-open-distribution.md); the factual comparisons remain research evidence.

## Trust models

| Product | Runtime boundary | Additional trust measures |
|---|---|---|
| Raycast | Managed Node runtime with extension workers; no additional filesystem/network sandbox in its published security model | Public extension review and build checks; OS privacy controls |
| Pi | Conventional extensions execute inside the host process with its OS privileges | Explicit source trust; project trust before project packages load |
| Desktop VS Code | Extension host inherits VS Code's permissions, including files, networking and external processes | Publisher trust prompts, Marketplace scanning, signatures and block lists |
| Obsidian | Community plugins inherit application access; no reliable per-plugin permission restriction | Restricted Mode prevents loading community plugins; directory review/scanning |
| Gauntlet | Deno extension processes with declared permissions | Best-effort security caveat; Git URL distribution |
| Chrome extensions | Browser APIs and host access governed by declared permissions | Optional permissions; warning-generating permission increases require acceptance |

Sources and qualifications:

- Raycast's security page is macOS-focused and should not be read as a complete audit of its Windows host. Worker separation and its limited host RPC API do not restrict independent Node APIs. [Raycast security](https://developers.raycast.com/information/security)
- Pi's shared-process access includes files, credentials and session information; a package may also contain non-code resources. [Pi extensions](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md), [package trust](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/packages.md)
- VS Code's official runtime document explicitly identifies file writes, network requests and processes as extension capabilities. Its Marketplace scans are distribution defenses; the clean-room scan environment is not the user's ordinary runtime sandbox. [VS Code runtime security](https://code.visualstudio.com/docs/configure/extensions/extension-runtime-security)
- Obsidian says installed community plugins remain present but ignored in Restricted Mode. This is a loading gate, not a restricted execution mode for each running plugin. [Obsidian plugin security](https://obsidian.md/help/plugin-security)
- Gauntlet is a useful launcher precedent but says security measures are best-effort; its repository also states development has stopped. [Architecture](https://gauntlet.sh/docs/information/architecture), [repository](https://github.com/project-gauntlet/gauntlet)
- Chrome supplies an example of capability enforcement and permission evolution, not a drop-in desktop runtime. Not every permission produces an install warning. [Permissions](https://developer.chrome.com/docs/extensions/develop/concepts/declare-permissions), [permission updates](https://developer.chrome.com/docs/extensions/develop/concepts/permission-warnings)

## Recommendation after the Wasm exploration

If we adopt WIT/Wasmtime, use a capability-based default. A component receives explicit host imports and per-extension grants rather than unrestricted native access. Provide a small set of useful services (own storage, selected filesystem locations, network destinations, current clipboard, application launch). Treat clipboard-history access separately from reading the current clipboard.

This is a deliberate conditional recommendation. Earlier trusted-code advice assumed a Node-like execution path. Wasm changes the implementation trade-off because external operations already cross host-provided imports. It does not eliminate the need to implement and verify those imports correctly. [Wasmtime sandbox model](https://docs.wasmtime.dev/security.html)

Start with one execution model. Do not build an unrestricted helper runtime merely as speculative future flexibility. Add a visibly different full-trust mode only when concrete workflows require it. Arbitrary shell access is not a small permission: the spawned program can access resources outside a language-runtime sandbox. Deno explicitly documents that limitation. [Deno subprocess permissions](https://docs.deno.com/runtime/reference/permissions/)

Review and provenance should complement enforcement. Identify extension publisher, exact installed version, source, content digest and review status. A cryptographic digest only detects changed content; it does not establish publisher identity or benign behavior. Git tags may be moved upstream, so record the resolved commit/artifact as well as the human-readable version. Source identity must not be inferred merely from a user-supplied display name.

Package installation is another boundary. A sandboxed `.wasm` guest does not protect users if installation first executes arbitrary npm scripts or repository build commands on the host. Recommend prebuilt portable components for ordinary installation. Local source builds belong to an explicit developer workflow, outside the guest sandbox unless separately isolated.

## Q10: is this how Raycast and Pi work?

**Yes for defining their own API; their distribution policies differ.** Raycast's public flow sends a pull request through review into its Store, while its developer tooling supports source import and bundling. Pi explicitly accepts npm, Git and local sources and provides resource discovery/selection. Both expect extensions written for their respective API; neither is a universal arbitrary-plugin loader. [Raycast publication](https://developers.raycast.com/basics/publish-an-extension), [Raycast CLI](https://developers.raycast.com/information/developer-tools/cli), [Pi packages](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/packages.md)

Our proposed combination is a discoverable catalog plus externally distributed packages, all using our one SDK. For Wasm, a GitHub/npm source would be a distribution source for our manifest and component artifacts, not permission to run that repository's arbitrary code during normal installation. Existing Raycast/Pi extension compatibility remains separate scope.

## Decision status

Research supports a concrete draft policy, but the user has not accepted a runtime, trust model, ecosystem compatibility scope or disable-data policy. See [policy proposal](../extension-policy-proposal.md) and [disable/data evidence](extension-disable-data.md). This research resolves facts and recommends defaults without recording them as accepted ADRs.
