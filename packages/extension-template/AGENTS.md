# Extension Template

## What this package is

A Pane extension: a WebAssembly component implementing the
`pane:extension` contract, written in TypeScript with the
`@pane-app/extension` SDK and built by the `@pane-app/cli` tool's
`pane-ext`. The package's manifest `pane.json` names its command and the
component serving it — `dist/extension-template.wasm`, which the release
build writes from `src/index.ts`; until then the package is source only,
and Pane installs nothing.

## Before writing code, read

- **The contract** — the WIT files that define the extension API, in
  Pane's repository:
  [wit/extension.wit](https://github.com/pane-app/pane/blob/main/wit/extension.wit)
  first, then `commands.wit`, `root-results.wit`, `operations.wit`,
  `search.wit`, `service.wit`, `data.wit`, `preferences.wit`,
  `feedback.wit` and `programs.wit` in the same
  [wit](https://github.com/pane-app/pane/tree/main/wit) folder.
- **The SDK's types** — after `npm install`, in
  `node_modules/@pane-app/extension/`: `pane.d.ts` and the per-module
  `.d.ts` files (`feedback`, `http`, `system`, `preferences`, `icons`,
  `programs`).
- **The examples** — Pane's own sample extensions, each in Rust, JavaScript
  and TypeScript, in Pane's
  [guests](https://github.com/pane-app/pane/tree/main/guests) folder:
  `sample-ts`, `sample-preferences-ts`, `sample-clipboard-ts`,
  `sample-operations-ts`, `sample-search-ts`, `sample-service-ts`, and the
  authoring guide
  [guests/README.md](https://github.com/pane-app/pane/blob/main/guests/README.md).
- **The docs** — Pane's
  [docs](https://github.com/pane-app/pane/tree/main/docs) folder:
  [development mode](https://github.com/pane-app/pane/blob/main/docs/development-mode.md)
  (what `npm run dev` does, and what a build failure shows),
  [the list tree](https://github.com/pane-app/pane/blob/main/docs/list-tree.md)
  (the tree a command's screen renders, and its icons and accessories),
  [operations](https://github.com/pane-app/pane/blob/main/docs/operations.md),
  [dependencies](https://github.com/pane-app/pane/blob/main/docs/dependencies.md),
  [helpers](https://github.com/pane-app/pane/blob/main/docs/helpers.md)
  (native helper programs), and
  [npm](https://github.com/pane-app/pane/blob/main/docs/npm.md) and
  [git](https://github.com/pane-app/pane/blob/main/docs/git.md) (how
  users install packages).

## The commands

- `npm install` — once, before anything else.
- `npm run dev` — build and hand the package to the running Pane, which
  reloads it on each save, until Ctrl+C. Its errors are the build's own
  (tsc, esbuild), shown in the terminal.
- `npm run check` — everything Pane would refuse at install, with Pane's
  own messages, plus this package's eslint.
- `npm run pack` — build the release components into `dist/` and check
  what users will download.
- `npx pane-ext new command . --template <kind>` — add a command to the
  package: the manifest entry, the source file and the dispatch arm. The
  entry file's comments mark where each goes.

## Releasing

A release tag `v<semver>` — the same version `pane.json` names — is what
CI turns into a release revision: it builds `dist/`, commits it and moves
the tag, so what the tag names is the revision Pane installs
(`git:github.com/<owner>/<repository>@v<semver>`). Pane builds nothing
from a repository, so a revision without the built components installs
nothing; the default branch stays source-only. See the repository's
README for the steps.
