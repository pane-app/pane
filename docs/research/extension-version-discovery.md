# Discovering available extension versions

**Q32 status update:** the user challenged the historical-version picker as unreliable. The recommendation below is historical and superseded: now recommend deferring automatic version-history browsing for launch, retaining accepted pins, and allowing explicit exact-version/commit source input where supported. The user subsequently accepted deferring the picker: "ye right now its not needed". Existing pins/source controls remain; no dedicated historical-version UI is committed. Source discovery facts remain useful; advertised versions are not a guarantee of installability or reversible data changes.

Q32 follow-up, 2026-09-27: user asks how the launcher would know which versions are available. Manual version selection remains proposed. These are existing source capabilities and a proposed adapter design, not implemented launcher behavior.

## npm registry metadata

The recorded registry can be queried directly over HTTP for its package metadata. The `versions` map supplies published version numbers and per-version metadata/download details; `dist-tags` such as latest are pointers, not the full history. This lookup requires no user-installed npm CLI or execution of extension code. Custom launcher metadata may require the full response or package-manifest inspection rather than the abbreviated install metadata. [Official registry metadata format](https://github.com/npm/registry/blob/main/docs/responses/package-metadata.md), [npm discovery audit](npm-version-discovery.md)

Proposed flow: request metadata for the installed package's recorded registry, present candidate versions, then validate selected launcher/API/platform/artifact requirements. Keep compatibility claims limited to checked declarations and artifact availability; source metadata cannot guarantee bug-free execution.

## Git and release hosting

Git can advertise remote tags and their object IDs through `ls-remote --tags`; listing advertised refs does not require cloning the entire repository. Tags are candidate revision labels, not proof of an installable extension release. Generic Git does not define binary release assets. [Git ls-remote](https://git-scm.com/docs/git-ls-remote)

For GitHub, the Releases API provides published release records and attached asset metadata/download URLs. It explicitly excludes ordinary Git tags with no associated GitHub release. Therefore a Git tag list and a GitHub release list are not interchangeable; other hosts require their own supported release API or an explicit package artifact location. [GitHub Releases API](https://docs.github.com/en/rest/releases/releases#list-releases)

Proposed approach: identify the original configured source, obtain release/tag candidates using its supported interface, and inspect the extension's declared compatibility/artifact metadata for a selected candidate before installation. A fixed Git pin resolves to a commit identity; a moving branch should be labeled as such. Artifact mapping is an extension package/SDK contract we still need to define, not something GitHub automatically understands.

## Local development

A local directory has no universal published-version history. Continue Q30's developer-managed behavior: users/developers change local files or their own checkout. Do not inspect every local Git history or offer automatic checkout as an implicitly accepted feature.

## What available means

A version advertised by a registry/tag list is a candidate. Ordinary installation additionally requires compatible launcher API, OS/architecture and dependencies plus a ready-to-run artifact. In particular a Rust source tag alone does not supply a Windows/macOS/Linux executable. JS/TS also needs supported packaging/dependencies. Show unavailable/unsupported status when those requirements cannot be established; do not promise that every historical version can install without a compiler.

No launcher-owned central registry is needed merely to discover versions from existing sources. Proposed source adapters can fetch/cache metadata when version management is opened, with pagination where applicable and detail validation on selection. Offline/stale caches cannot prove an artifact is still downloadable; an empty result, authentication failure and network error should be distinct. Exact caching, credentials and source adapter scope are implementation work, not a reason to invent a mandatory always-running discovery service.

## Q32 recommendation, still pending

Offer a version list only where the source exposes a useful list, and allow explicit Git ref input as an advanced path subject to the same packaging constraints. Describe known candidates honestly; verify selected metadata/artifact availability before activation. Keep a specific-version change within the tracked source installation, preserving the Q28/Q29 identity rules. No automatic data migration reversal or guarantee that publishers retain historical downloads follows from this design.
