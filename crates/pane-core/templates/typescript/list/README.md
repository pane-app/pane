# __TITLE__

A Pane extension in TypeScript, started from the `list` template.

Its command opens a list of items, and each item runs an action when
chosen. `Say hello` shows a toast, as every item's action tells the user
what it did. The list is `theList` in `src/index.ts`. Edit it and save,
and Pane rebuilds and reloads the command while it keeps running.

## Trying it

With Node.js and npm installed:

```
npm install
npm run dev
```

`npm run dev` runs `pane-ext dev`. It builds the package (its TypeScript
and esbuild, then the WebAssembly component) and hands the build to the
running Pane, starting one if none is running. After each save it builds
the package again and has Pane reload it, until Ctrl+C. Pane's
development-mode documentation describes what happens when a save does
not build.

Two more scripts:

- `npm run check` reports what Pane would refuse at install.
- `npm run pack` builds the release components and assembles what the
  package's users will download.

## What is here

- `pane.json`: the package's manifest, with its title, its icon and its
  commands. Its `"$schema"` points at the schema the
  [`@pane-app/extension`] SDK ships, so an editor that knows JSON Schema
  checks the file once `npm install` has run. Pane itself ignores fields
  it does not know.
- `src/index.ts`: the command, written with Pane's TypeScript SDK. One
  component (the WebAssembly file `pane.json` names) serves every command
  the package declares. `render` and `run` receive the command's id, so
  the file matches on it. Add a command with `pane-ext new command .`.
- `package.json`: the package and its devDependencies. Those are the SDK
  ([`@pane-app/extension`], types and runtime), the tool
  ([`@pane-app/cli`], whose `pane-ext` the scripts run), the build's
  esbuild and TypeScript, and the linter and formatter. In a Pane
  checkout, point the two `@pane-app` devDependencies at the repository's
  own copies (`"file:../guests/js"`) to build against the SDK as it is
  there, with changes not yet published.
- `tsconfig.json`: the type checker's settings, as Pane's own TypeScript
  samples set them.
- `biome.json`: the linter's and the formatter's settings.
  `npx biome check .` lints and `npx biome format --write .` formats.
  Biome needs no peer of TypeScript's, so it installs beside the version
  the build pins.
- `icon.png`: the package's placeholder icon, 512×512. Replace it with
  your own.
- `.gitignore`: keeps `node_modules/` and `dist/`, the build's output,
  out of the repository.

[`@pane-app/extension`]: https://www.npmjs.com/package/@pane-app/extension
[`@pane-app/cli`]: https://www.npmjs.com/package/@pane-app/cli
