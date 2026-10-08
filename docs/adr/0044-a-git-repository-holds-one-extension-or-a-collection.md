# A Git repository holds one extension or a collection of them

Accepted 2026-10-08 by the user's decision ("maybe we gotta support handle repo with many extensions as well?"), after research into how other ecosystems install from a repository holding many installable things: Claude Code plugin marketplaces, Home Assistant add-on repositories, pre-commit, Zed's registry, Go modules, Helm, Pi packages, GitHub Actions, Neovim plugin managers, Obsidian and HACS. The shape below is accepted; the details marked *proposed* were delegated and stay open to change by the specification that implements them.

Until now a Git-distributed package keeps its `pane.json` at the repository's root, so a repository is one extension ([ADR 0021](0021-pane-fetches-git-packages-itself.md) left "a package in a subfolder of a repository" open, and [ADR 0012](0012-pi-style-source-identity.md) left multi-resource packaging open). One extension already holds many commands; a collection is for an author who keeps several extensions in one repository.

**Two shapes.** A repository (or local folder) whose root holds `pane.json` is one extension, as today. One whose root holds `pane-collection.json` is a **collection**; a root holding both is refused. The collection lists its extensions explicitly:

```json
{
  "extensions": [
    { "id": "clock", "path": "extensions/clock" },
    { "id": "timers", "path": "extensions/timers" }
  ],
  "renamed": { "old-clock": "clock", "retired-thing": null }
}
```

Each `id` (lowercase letters, digits and hyphens) is unique within the collection and kept across its releases. Each `path` is a folder holding an ordinary extension package: its own `pane.json`, built components and helpers. The index holds no titles or versions: those are each package's own manifest's, so the two cannot drift. `renamed` maps an id that was renamed to its new id, or to `null` for one that was removed. An extension's folder must hold everything it needs: nothing outside it is installed, so a component reached through `../` is missing and the package is explained as source-only.

**Installing.** `git:<repository>` naming a collection shows its extensions (each package's title, description and version) to install some or all of them; `git:<repository>#<id>` installs one, with a reference after it as ADR 0021 reads one (*proposed*: `git:github.com/owner/tools#clock@refs/tags/clock/v1.2.0`). Every extension chosen is installed on its own: its own install preview ([ADR 0002](0002-trusted-extensions-and-open-distribution.md) grants no permissions; the preview is where the user sees what it uses), its own managed copy, record, updates, disabled state, uninstall and data. Choosing several is a convenience, not a unit: one that cannot be installed is explained, and the others are installed (*proposed*). A local folder holding `pane-collection.json` installs the same way. One npm package remains one extension; a collection on npm stays open.

**Identity.** An extension of a collection is `git:<host>/<path>#<id>`, the repository normalized as ADR 0021 normalizes it; this amends ADR 0012. The id, not the folder, names it, so moving its folder keeps its identity, and an id followed through `renamed` keeps it too. Turning a one-extension repository into a collection, or back, changes identity, as moving a source does under ADR 0012.

**Versions and updates.** Each extension's version is its own manifest's. A one-extension repository tags its releases `v<semver>`; a collection tags each extension's releases `<id>/v<semver>`, Go's convention for modules in subfolders, so each extension has a release history of its own and no release of one forces a release of the others. Checking a collection's extension for an update looks for its highest stable `<id>/v…` tag above the installed version, checks that the revision's index still lists the id, and pins the commit the tag resolves to (*proposed*). A tracked branch is fetched again as ADR 0021 decides. An id that is gone, or renamed to `null`, is reported to the user and the installed extension keeps running; it is never uninstalled silently.

**Fetching** (*proposed*). Where the host supports Git's partial-clone filter, Pane fetches the revision's trees without file contents and then only the files under the chosen extensions' folders; otherwise it fetches the whole revision within ADR 0021's limits. Extensions installed from one revision share one fetch.

**Rejected.** Scanning the repository for `pane.json` files instead of an index: no stable ids, and test fixtures and samples would be offered as extensions; every ecosystem that holds many things per repository and lets the user pick among them lists them explicitly. Identifying an extension by its folder path (Terraform's `//subdir`): folders move, ids need not. Tags for the whole repository (GitHub Actions' model): every extension's release would move together, and a breaking change to one would be a breaking release of all. Asking authors to mirror each extension into a repository of its own (mini.nvim's workaround for plugin managers that cannot install a subfolder).
