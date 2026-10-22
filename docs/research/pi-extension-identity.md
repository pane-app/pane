# Pi package and extension identity

**Decision update (Q29):** the user chose "ok do like pi" after the comparison. Pi-style source-derived package identity is accepted; earlier namespaced-ID and UUID recommendations below are historical and superseded. See [ADR 0012](../adr/0012-pi-style-source-identity.md). Exact development override and resource/operation addressing remain open.

Research for Q28/Q29, 2026-09-27. Conventional Pi coding-agent packages/extensions at commit `2b0a123de98318c2ff8069661721ce0c3794c34e`; not the separate Chord runtime. Source inspection only; no third-party code executed.

## Package identity comes from the installation source

Pi documents three package identities: npm package name, Git repository URL excluding its ref, and resolved absolute local path. Packages can contain several extension resources plus skills, prompts, and themes. The package's npm `name` is therefore not a mandatory standalone identity for each extension resource. [Package documentation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/docs/packages.md#understand-scope-and-identity)

`getPackageIdentity` implements these as `npm:<package name>`, `git:<host>/<repository path>`, and `local:<resolved path>`. Git host/path normalization treats SSH and HTTPS forms of the same repository alike. Versions/refs do not form part of the identity. Project declarations normally replace personal declarations with the same identity; a project entry with `autoload: false` instead filters the personal package. [Package identity and deduplication implementation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/package-manager.ts#L1569)

Consequently, this mechanism alone does not recognize an npm release and a separately cloned local/Git copy as the same extension: their package identity prefixes differ. This is an inference from the identity function, not a claim that both always load regardless of resource filtering.

## Resource paths and command names are separate identities

The resource loader merges extension paths by their canonical filesystem paths, skipping repeated references to the same path. In-memory extension factories receive synthetic paths such as `<inline:name>`. This is path-based resource tracking, not a universal author-and-extension namespace. [Resource loader](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/resource-loader.ts#L858)

Command names are also separate: when multiple loaded extensions register the same command name, the runner generates invocation names with numbered suffixes, such as `name:1` and `name:2`. Tools use a different policy: the first registration of a tool name wins in the runner's collected tool list. Neither behavior establishes an extension-wide publisher namespace. [Command resolution](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/extensions/runner.ts#L693), [tool collection](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/extensions/runner.ts#L552)

## Implication for our launcher

Adopting Pi's npm/Git/local distribution does not require copying its identity scheme. A developer-declared stable extension ID, independent of installation source, would serve our accepted duplicate-ID rejection and cross-extension calling requirements. This is our design proposal, not an existing Pi feature. Store the source separately to determine where updates come from. Exact ID syntax and any publisher ownership rules remain to be decided by the parent design discussion.
