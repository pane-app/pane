# npm version discovery

Researched 2026-09-27 for Q32. Research only; manual version selection remains a proposal.

## Discovery

The launcher can request `GET https://registry.npmjs.org/:package` directly over HTTP. npm documents this endpoint as returning a JSON package metadata document (a packument). This does not require invoking the npm CLI or executing package code. [Registry API](https://github.com/npm/registry/blob/main/docs/REGISTRY-API.md#package-endpoints)

The response's `versions` map supplies published version numbers and their metadata. `dist-tags` maps labels such as `latest` to versions; these labels are pointers, not the complete version history. Each version's `dist` supplies its tarball URL and checksum information, including `integrity` where present. The abbreviated response, requested with `Accept: application/vnd.npm.install-v1+json`, includes installation metadata, declared platform/runtime constraints and deprecation messages. Full metadata also contains publisher-defined manifest fields and publication timestamps. [Package metadata format](https://github.com/npm/registry/blob/main/docs/responses/package-metadata.md)

## Compatibility limits

`os` and `cpu` express declared platform restrictions; `libc` can narrow Linux support. `engines.node` declares supported Node versions and is normally advisory in npm unless strict enforcement is configured. Native npm packages may compile during installation. These fields therefore do not establish that a package contains a ready-to-run artifact for every supported launcher target. [npm package manifest](https://docs.npmjs.com/cli/v11/configuring-npm/package-json/#os), [engines](https://docs.npmjs.com/cli/v11/configuring-npm/package-json/#engines), [default native build behavior](https://docs.npmjs.com/cli/v11/configuring-npm/package-json/#default-values)

## Implication for the launcher

Proposed flow: query metadata for the installed package's recorded registry/source, present its available versions, and validate launcher API, runtime and target compatibility before replacement. Listing versions needs only an HTTP client. Custom launcher compatibility fields may require full version metadata or inspecting the package manifest, because npm's abbreviated representation uses an allowlist. A listed version is a discovery result, not a guarantee that installation or startup will succeed.

Q32's exact interface and compatibility policy are not accepted by this research note. No package was installed or executed.
