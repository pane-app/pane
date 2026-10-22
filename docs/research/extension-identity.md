# Who defines an extension ID?

**Decision update (Q29):** the user chose "ok do like pi" after the comparison. Pi-style source-derived package identity is accepted; earlier namespaced-ID and UUID recommendations below are historical and superseded. See [ADR 0012](../adr/0012-pi-style-source-identity.md). Exact development override and resource/operation addressing remain open.

Research for Q29, 2026-09-27. Q28 duplicate-ID rejection is accepted; ID format and namespace policy remain proposed. The user asks whether the developer defines the ID. This is source/documentation research, not an implemented manifest or installer.

## Existing systems

VS Code authors declare `publisher` and `name` in the manifest; the extension identifier is their combination, `publisher.name`. Its Marketplace publishing flow associates the publisher identifier with a publisher account. The editable display name is separate from the extension identity. [Extension anatomy](https://code.visualstudio.com/api/get-started/extension-anatomy), [publisher setup](https://code.visualstudio.com/api/working-with-extensions/publishing-extension#create-a-publisher)

Raycast declares an extension `name`, display `title`, and account `author`/organization `owner`. Its cross-extension command addressing includes owner/author name, extension name and command name. Do not assume its Store namespace/uniqueness rules equal an unregistered namespaced-ID scheme. [Manifest](https://developers.raycast.com/information/manifest), [command targets](https://developers.raycast.com/api-reference/command#interextensionlaunchoptions)

npm uses scoped package names such as `@author/package`; user/organization accounts own scopes in its registry. That is registry package identity, not automatically our extension ID for every Git/local/Rust package. [npm scopes](https://docs.npmjs.com/about-scopes/)

Pi's conventional package/source identity is a different model: see [Pi identity audit](pi-extension-identity.md). A package identity or file path should not be presented as a universal stable author-defined extension ID without checking the actual resolver.

## Earlier namespaced-ID proposal, challenged by the user

Have the author declare a stable namespaced ID, such as `acme.file-search`, in the extension manifest. The scaffold can ask for an author/project namespace and extension slug, then fill it in. This is a proposed manifest fragment, not an implemented schema:

```json
{
  "id": "acme.file-search",
  "title": "File Search",
  "version": "1.0.0"
}
```

The namespace is an author/team/project label; it need not be a person's current account username. A second author can use `otherteam.file-search` without colliding. Keep the ID stable across releases, implementation languages and npm/Git/local distribution. Version and display title are separate fields; changing the title does not create a new extension. A fork that should coexist uses a distinct ID.

The installer validates the format and rejects a second installation of an existing full ID, as accepted in Q28. Updates remain tied to the recorded installation source and selected update policy; an arbitrary package claiming the same ID is not automatically an update. The exact normalization/character rules, ID changes, ownership transfers and local development override behavior need design.

Under the accepted open npm/Git/local distribution model, the proposal does not require a launcher account or central publisher registry. A self-declared namespace reduces accidental collisions but cannot guarantee global uniqueness or prove publisher ownership. Source location/provenance and extension identity must be recorded separately. This is a factual limitation of decentralised naming, not a proposal to add a permission sandbox or publisher-review gate.

The extension ID is also distinct from a command/operation ID. Programmatic targets need both extension and operation identity; exact combined syntax and version negotiation remain part of the SDK design. A source-independent ID alone is not enough to locate a missing dependency across multiple registries: dependency source resolution remains separate implementation work.

## Revised Q29 recommendation: generated permanent identity (pending)

The user objects that developer-chosen names can duplicate. The earlier namespace proposal is superseded as the recommendation, not an accepted decision. Recommend scaffolding a UUIDv4 once using OS-backed cryptographic randomness and storing it in the manifest. Keep a readable title/slug separately. Generate once per new extension, not per build, release, installation or target platform.

```json
{
  "id": "fe589a8c-30ac-49d3-a844-c739e977f210",
  "name": "file-search",
  "title": "File Search"
}
```

This fragment illustrates a proposed schema only. UUIDv4 uses 122 random bits; RFC 9562 recommends cryptographically secure randomness and explicitly distinguishes practical uniqueness from an absolute global guarantee. Accidental independent collisions are negligible with correct generation. [UUIDv4](https://www.rfc-editor.org/rfc/rfc9562.html#section-5.4), [uniqueness](https://www.rfc-editor.org/rfc/rfc9562.html#section-6.8), [randomness](https://www.rfc-editor.org/rfc/rfc9562.html#section-6.9)

Preserve the ID across updates, language changes and distribution sources. Two independently scaffolded extensions can share a readable name and remain distinct. A copied manifest retains its ID: copying is not an independent random collision. A fork intended to coexist must generate a new ID; a second same-ID install is rejected under accepted Q28. Normal updates remain tied to the tracked installation/source. A UUID does not authenticate its publisher or locate a dependency for download.

No registry account is required. Tooling should expose readable labels and resolve selections to IDs, so users do not routinely type UUIDs. SDK/dependency declarations retain the actual stable ID, with source resolution recorded separately. This trades readable canonical IDs for decentralised collision resistance; exact tooling and dependency-source schema remain open. Await the user's Q29 answer before treating this as policy or writing an accepted ADR.

## Q29 follow-up: what existing systems actually prevent

User requested comparison with Pi, Raycast and other extension systems before deciding. UUIDv4 remains a proposal; it must not be described as the way those products require authors to identify extensions.

### Pi: package identity follows the source

Rechecked the pinned source and docs. Pi identifies npm packages by package name, Git packages by normalized host/repository path without the ref, and local packages by resolved absolute path. Equivalent personal/project package declarations are deduplicated (normally project wins). The package can contain several extension resources. This handles repeated source declarations, not universal identity across every distribution channel. An npm package and its Git/local copy receive different package identities; that follows directly from the identity function. [Pi package docs](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/docs/packages.md#understand-scope-and-identity), [identity implementation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/package-manager.ts#L1569)

### VS Code: readable identity with a registered publisher

The manifest supplies publisher and name, forming `publisher.name`. Marketplace publisher creation reserves a unique publisher identifier, and publishing requires authorization for that publisher. Thus `alice.search` and `bob.search` can be distinct identities, with namespace ownership enforced by the Marketplace publishing service. The name format alone supplies no equivalent ownership guarantee for arbitrary external manifests. This last point is a limitation of self-declared metadata, not a claim about a tested sideload collision dialog. [Extension identity](https://code.visualstudio.com/api/get-started/extension-anatomy#extension-manifest), [publisher registration and authentication](https://code.visualstudio.com/api/working-with-extensions/publishing-extension#create-a-publisher)

npm offers an analogous existing authority for scoped package identities: registering a user or organization supplies its scope. This covers npm's registry namespace, not all possible local/Git extension distributions. [npm scopes](https://docs.npmjs.com/about-scopes/)

### Raycast: named extensions within a managed publishing flow

Raycast's manifest defines `name` as unique, separates the display `title`, and uses the author's Raycast Store handle (or an organization owner). Cross-extension calls address author/owner plus extension name and command. Public publishing opens a pull request into Raycast's extensions repository; Raycast reviews and merges it before Store publication. Thus names participate in an account-backed, coordinated publishing workflow. The public manifest does not require an author-generated permanent UUID. [Manifest](https://developers.raycast.com/information/manifest), [command addressing](https://developers.raycast.com/api-reference/command#interextensionlaunchoptions), [publishing flow](https://developers.raycast.com/basics/publish-an-extension)

Do not infer exact global name uniqueness across public, private and local extensions, or the closed Store database's internal ID format, from the word unique or the public repository layout. See the separate [Raycast collision audit](raycast-identity-collisions.md) for the inspected CLI's name-and-author lookup and limitations. No local Raycast duplicate-install experiment was run.

### Implication for the pending decision

There are distinct tradeoffs: Pi's source-derived identity requires no launcher-owned name registry but changing distribution location can change identity; a registered publisher/name scheme provides readable coordinated names but introduces a registry authority; a generated permanent UUID provides practical collision resistance and stable identity across sources without that authority. UUIDs do not deduplicate copied source code with newly generated IDs, prevent deliberate copying of an ID, or prove authorship. Same readable titles are possible with all three designs and should be disambiguated in the UI with author/source metadata.

Given the accepted open npm/Git/local distribution and desired cross-extension references, the current recommendation remains generated permanent IDs plus readable names and separate recorded sources. This is our design judgment, not a copied Pi/Raycast implementation, and awaits acceptance. Pi-style source identity is a simpler alternative if identity changing with distribution source is acceptable.
