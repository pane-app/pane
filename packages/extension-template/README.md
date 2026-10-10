# Extension Template

Pane's template repository for extensions: a working TypeScript package
(what `pane-ext new` writes from the `list` template), with CI that builds
and checks it on every push and turns every `v<semver>` tag into a release
revision Pane installs. **Use this template** (GitHub's button above) to
start your own extension, then make it yours
([below](#making-it-yours)).

The package is one command, "Extension Template", which opens a list of
items, each running an action when chosen: `Say hello` shows a toast, as
every item's action tells the user what it did. The list is `theList` in
`src/index.ts`; edit it and save, and Pane rebuilds and reloads the command
while it keeps running.

## What is here

- `pane.json` — the package's manifest: its title, its icon and its
  commands. Its `"$schema"` points at the schema the
  [`@pane-app/extension`] SDK ships, so an editor that knows JSON Schema
  checks the file once `npm install` has run; Pane itself ignores fields it
  does not know.
- `src/index.ts` — the command, written with Pane's TypeScript SDK. One
  component (the WebAssembly file `pane.json` names) serves every command
  the package declares: `render` and `run` receive the command's id, so the
  file matches on it. Add a command with `pane-ext new command .`.
- `package.json` — the package and its devDependencies: the SDK
  ([`@pane-app/extension`], types and runtime), the tool ([`@pane-app/cli`],
  whose `pane-ext` the scripts run), and the build's esbuild and TypeScript,
  the linter and the formatter.
- `tsconfig.json`, `eslint.config.js`, `.prettierrc.json` — the
  type-checker's, linter's and formatter's settings; `npx prettier -w .`
  formats the sources.
- `icon.png` — the package's placeholder icon, 512×512 (a published
  extension's icon size); replace it with your own.
- `AGENTS.md` — what an AI asked to write the extension should read first.
- `.github/workflows/ci.yml` — the CI: [Releasing](#releasing) describes
  what it does with a tag.
- `.gitignore` — keeps `node_modules/` and `dist/`, the build's output, out
  of the repository; a release revision commits `dist/` anyway, on purpose
  (see below).
- `LICENSE-APACHE`, `LICENSE-MIT` — the package's dual license; keep or
  change it as you like, with the matching `license` fields in
  `pane.json` and `package.json`.

## Trying it

With Node.js and npm installed:

```
npm install
npm run dev
```

`npm run dev` runs `pane-ext dev`, which builds the package (its
TypeScript and esbuild, then the WebAssembly component), hands the build to
the running Pane (starting one if none is running), then builds it again
after each save and has Pane reload it, until Ctrl+C.

`npm run check` reports what Pane would refuse at install — with Pane's own
messages — plus the package's own eslint, so a broken release fails in CI
before it is made. `npm run pack` builds the release components and checks
what the package's users will download.

## Releasing

Pane never builds a package from a repository: what its users install is a
**release revision**, a commit whose tree holds the built components
`pane.json` names. This repository's CI makes one from a tag:

1. Set the version in `pane.json` (and `package.json`) and push the
   commit.
2. Tag it `v<semver>` — the same version the manifest names — and push the
   tag.
3. CI builds `dist/`, commits it on the tagged commit and moves the tag
   onto the new commit, so what the tag names is the release revision. (It
   will run the workflow a second time on the moved tag, which then finds
   the release revision already there and stops.)

Users install the package from Pane's root search, "Install extension from
Git…", with the repository's address and the tag —
`git:github.com/<owner>/<repository>@v0.1.0` — or by naming the repository
alone, which tracks its default branch (so keep that branch source-only and
release through tags, as this template does).

See Pane's [Git-distributed packages] documentation for what users see.

## Making it yours

- `pane.json`: the `title`, `description`, `version`, `repository`,
  `issues` and `keywords`, and the command's `id`, `title`, `subtitle`.
- `package.json`: the `name` and `description`.
- `src/index.ts`: the `GREETING`, the command's id in the dispatch, and the
  list itself.
- `icon.png`: your own 512×512 icon.
- The license, if the dual Apache/MIT one is not yours.

Or start over: `npx @pane-app/cli new` (or `npm create @pane-app`) writes a
fresh package from the same templates this repository's package came from,
in TypeScript or Rust.

[`@pane-app/extension`]: https://www.npmjs.com/package/@pane-app/extension
[`@pane-app/cli`]: https://www.npmjs.com/package/@pane-app/cli
[Git-distributed packages]: https://github.com/pane-app/pane/blob/main/docs/git.md
