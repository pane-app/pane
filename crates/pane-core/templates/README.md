# The templates `pane-ext new` writes

This folder holds the four templates
[ADR 0047](../../../docs/adr/0047-extensions-are-built-from-the-app-first-with-pane-ext-beside-it.md)
settles (`list`, `detail`, `form` and `no-view`) in Rust and in
TypeScript. [`crate::templates`](../src/templates.rs) embeds these files
with `include_str!`, so what `pane-ext new` writes is what this repository
holds. The same will hold for the app's Create Extension command, when it
lands.

A change to a source file here changes the scaffold, and the templates
cannot drift from what is committed. The same crate already embeds the
JS/TS SDK this way (`crates/pane-build`'s `js_assets`).

## Layout

### One folder per template

- `rust/<kind>/` is a Rust template:
  - `Cargo.toml`, a standalone package depending on the `pane-extension`
    crate from crates.io
  - `pane.json`, with `"$schema"` pointing at the schema committed on
    Pane's main branch, since a Rust package has no `node_modules` to hold
    the SDK's copy
  - `README.md`
  - `src/lib.rs`, the command
- `typescript/<kind>/` is a TypeScript template:
  - `package.json`, private, with `dev`, `check` and `pack` scripts that
    run `pane-ext`. Its devDependencies are `@pane-app/extension` and
    `@pane-app/cli` from npm, plus the build's esbuild and TypeScript and
    the linter and formatter.
  - `pane.json`, with `"$schema"` pointing at the schema the
    `@pane-app/extension` devDependency puts in `node_modules`
  - `README.md`
  - `src/index.ts`

### Shared files

- `rust/` and `typescript/`, beside the kinds, hold the files every
  template of that language writes unchanged: the toolchain, formatter,
  linter and type-checker configuration, and `.gitignore`.
- `rust/command/` and `typescript/command/` hold the command files
  `pane-ext new command` writes. Each has the same command logic as the
  entry file's, as a module the entry file's dispatch calls.

### The icon

The placeholder `icon.png` is not a file here. `crate::templates` writes
it as a deterministic 512×512 PNG (a published extension's icon size,
`icons::PUBLISHED_ICON_SIZE`) through `icons::encode_png`. Every scaffold
carries the same bytes without a binary asset in the repository.

### Placeholders

The scaffold substitutes these in the files with the name the author
gave:

| Placeholder | Becomes | Spelled in |
| --- | --- | --- |
| `__TITLE__` | the title | Title Case |
| `__NAME__` | the package and command name | kebab case |
| `__CRATE__` | the Rust crate's artifact name | snake case |
| `__STRUCT__` | the Rust type | Pascal case |

## The entry files keep their dispatch

One component serves every command a package declares (`render` and
`run` receive the command's id), so each template's entry file matches on
the command id. A comment marks where `pane-ext new command` adds an arm
(a view command's in `render`, a no-view command's in `run`) and where it
adds the module itself.

The parameters the dispatchers keep for those arms are named with a
leading underscore, so they do not warn while unused.

A form command is a designed view, and a package's entry file names one
single designed view. So `pane-ext new command` cannot add a form
command. It refuses that kind and points at the `form` template instead.

## Keeping the templates checked

- `cargo xtask ci-lints` runs `cargo fmt --check` in each Rust template
  folder, so the Rust sources stay as rustfmt writes them.
- `pane-core`'s unit tests scaffold every template into a temporary
  folder. They check the file set, the manifest (through Pane's own
  reading of it), the icon, and `pane-ext new command`'s edits.
- `pane-ext`'s integration tests run `pane-ext new` as a process. They
  build and install every template in both languages through Pane's own
  build and launcher, and open each command. This is the CI seam for
  "every template is built and installed" (#221), in the ordinary test
  tiers.

Those tests point the templates' dependencies at this repository's own
copies, as the development samples do. So each template builds against
the SDKs it ships beside, not the last ones published. The templates
themselves name the registry packages an author installs.
