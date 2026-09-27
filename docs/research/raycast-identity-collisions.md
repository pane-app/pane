# Raycast extension identity and name collisions

**Decision update (Q29):** the user chose "ok do like pi" after the comparison. Pi-style source-derived package identity is accepted; earlier namespaced-ID and UUID recommendations below are historical and superseded. See [ADR 0012](../adr/0012-pi-style-source-identity.md). Exact development override and resource/operation addressing remain open.

Researched 2026-09-27 for Q29. Read-only review of official documentation and the published `@raycast/api` 2.5.2 CLI package. No extension code or CLI commands executed.

## Documented identity

Developers declare `name`, `title`, and `author` in `package.json`. Raycast describes `name` as unique and URL-compatible; `title` is the readable label. `author` is the developer's Raycast account handle. An organization can be specified with `owner`. The documented manifest does not require developers to supply a permanent UUID. The manifest's wording alone does not establish whether uniqueness is global or scoped across every public, organization, and local installation. [Manifest](https://developers.raycast.com/information/manifest), [Store preparation](https://developers.raycast.com/basics/prepare-an-extension-for-store)

Cross-extension command addressing explicitly takes `ownerOrAuthorName`, `extensionName`, and the command `name`. Thus the public addressing scheme includes a publisher/organization name, rather than relying on the extension's display title. This is not evidence that arbitrary users can claim someone else's account namespace. [Command API](https://developers.raycast.com/api-reference/command#interextensionlaunchoptions), [published TypeScript declarations, lines 6088–6103](https://unpkg.com/@raycast/api@2.5.2/types/index.d.ts)

Public Store publishing uses a GitHub pull request into `raycast/extensions`, followed by Raycast review and publication after acceptance. Consequently it has central account/review infrastructure; copying manifest text is not equivalent to publishing an accepted Store entry. [Publishing guide](https://developers.raycast.com/basics/publish-an-extension)

## What the published CLI actually checks

In `@raycast/api` 2.5.2, `dist/utils/publish/publish-to-public-repo.js` contains a helper (minified name `Fu`) that searches the public extensions repository for `package.json` files matching **both manifest name and author**. No matches gives the default destination `extensions/<name>`; one match reuses its containing directory; multiple matches throw an error. This shows both a default flat directory convention and a name-plus-author lookup. It does **not** justify a blanket claim that the Store allows, or rejects, all duplicate bare extension names across publishers. A new name colliding with an existing repository directory necessarily conflicts with that default destination, but exact server-side acceptance rules were not audited. [Version-pinned published CLI](https://unpkg.com/@raycast/api@2.5.2/dist/utils/publish/publish-to-public-repo.js), [official npm tarball](https://registry.npmjs.org/@raycast/api/-/api-2.5.2.tgz)

The documented development command imports an extension when needed. The documentation examined does not explain the exact collision behavior of two local projects with matching identity. Do not assert that it rejects, replaces, or automatically renames them. No claim is made about inaccessible Store database keys or internal UUIDs. [CLI documentation](https://developers.raycast.com/information/developer-tools/cli)

## Implication for this launcher

Raycast is evidence for readable author/organization-plus-name addresses supported by an account and publishing service. It is not evidence that a freely self-declared author prefix, without an authority, guarantees uniqueness. UUID identity for this launcher's open npm/Git/local distribution remains a separate proposed design; Raycast's documented authoring format does not establish that precedent.
