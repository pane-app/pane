# The templates `pane-ext new` writes

The four templates ADR 0047 settles — `list`, `detail`, `form` and
`no-view` — in Rust and in TypeScript, kept here as the files
[`crate::templates`](../src/templates.rs) embeds with `include_str!`, so
what `pane-ext new` (and the app's Create Extension command, when it
lands) writes is what this repository holds: a change to a source file
changes the scaffold, and the templates cannot drift from what is
committed. The same crate already embeds the JS/TS SDK this way
(`crates/pane-build`'s `js_assets`).

## Layout

- `rust/<kind>/` — a Rust template: `Cargo.toml` (a standalone package
  depending on the `pane-extension` crate from crates.io), `pane.json`
  (with `"$schema"`, pointing at the schema committed on Pane's main
  branch, since a Rust package has no `node_modules` to hold the SDK's
  copy), `README.md`, and `src/lib.rs`, the command.
- `typescript/<kind>/` — a TypeScript template: `package.json` (private,
  with `dev`, `check` and `pack` scripts that run `pane-ext`, and
  devDependencies on `@pane-app/extension` and `@pane-app/cli` from npm,
  plus the build's esbuild and TypeScript and the linter and formatter),
  `pane.json` (with `"$schema"` pointing at the schema the
  `@pane-app/extension` devDependency puts in `node_modules`), `README.md`
  and `src/index.ts`.
- `rust/` and `typescript/` besides the kinds hold the files every
  template of that language writes unchanged: the toolchain, formatter,
  linter and type-checker configuration, and `.gitignore`.
- `rust/command/` and `typescript/command/` hold the command files
  `pane-ext new command` writes: the same command logic as the entry
  file's, as a module the entry file's dispatch calls.

The placeholder `icon.png` is not a file here: `crate::templates` writes
it as a deterministic 512×512 PNG (a published extension's icon size,
`icons::PUBLISHED_ICON_SIZE`) through `icons::encode_png`, so every
scaffold carries the same bytes without a binary asset in the repository.

`__TITLE__`, `__NAME__`, `__CRATE__` and `__STRUCT__` in the files are
the placeholders the scaffold substitutes with the name the author gave:
the title in Title Case, the package and command name in kebab case, the
Rust crate's artifact name in snake case, and the Rust type in Pascal
case.

## The entry files keep their dispatch

One component serves every command a package declares (`render` and `run`
receive the command's id), so each template's entry file matches on the
command id, with a comment marking where `pane-ext new command` adds an
arm — a view command's in `render`, a no-view command's in `run`, a
form's in `submit_form`/`submitForm` — and where it adds the module
itself. The parameters the dispatchers keep for those arms are named with
a leading underscore, so they do not warn while unused.

## Keeping the templates checked

- `cargo xtask ci-lints` runs `cargo fmt --check` in each Rust template
  folder, so the Rust sources stay as rustfmt writes them.
- `pane-core`'s unit tests scaffold every template into a temporary
  folder and check the file set, the manifest (through Pane's own
  reading of it), the icon, and `pane-ext new command`'s edits.
- `pane-ext`'s integration tests run `pane-ext new` as a process, and
  build and install every template in both languages through Pane's own
  build and launcher, opening each command: the CI seam for "every
  template is built and installed" (#221), in the ordinary test tiers.

Until the SDKs are published (#281), those tests point the templates'
dependencies at this repository's own copies, as the development samples
do; the templates themselves name the registry packages an author
installs.
