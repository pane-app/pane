# __TITLE__: a Pane extension in TypeScript, started from the `form` template.

A command whose item opens a form: a name to type and a greeting to
choose, with a `Greet` button. Submitting it calls `submitForm` in
`src/index.ts`, which answers the result — or the problem with a field,
shown next to it. Edit the form or its answer and save, and Pane rebuilds
and reloads the command while it keeps running.

## What is here

- `pane.json` — the package's manifest: its title, its icon and its
  commands. Its `"$schema"` points at the schema the
  [`@pane-app/extension`] SDK ships, so an editor that knows JSON Schema
  checks the file once `npm install` has run; Pane itself ignores fields
  it does not know.
- `src/index.ts` — the command, written with Pane's TypeScript SDK. One
  component (the WebAssembly file `pane.json` names) serves every command
  the package declares: `render` and `run` receive the command's id, so
  the file matches on it. Add a command with `pane-ext new command .`.
- `package.json` — the package and its devDependencies: the SDK
  ([`@pane-app/extension`], types and runtime), the tool
  ([`@pane-app/cli`], whose `pane-ext` the scripts run), the build's
  esbuild and TypeScript, and the linter and formatter. In a Pane
  checkout, point the two `@pane-app` devDependencies at the repository's
  own copies (`"file:../guests/js"`) until they are published.
- `tsconfig.json` — the type checker's settings, as Pane's own
  TypeScript samples set them.
- `eslint.config.js` — the linter's settings, which `npm run check` runs
  through the package's own eslint.
- `.prettierrc.json` — the formatter's settings; `npx prettier -w .`
  formats the sources.
- `icon.png` — the package's placeholder icon, 512×512; replace it with
  your own.
- `.gitignore` — keeps `node_modules/` and `dist/`, the build's output,
  out of the repository.

## Trying it

With Node.js and npm installed:

```
npm install
npm run dev
```

`npm run dev` runs `pane-ext dev`, which builds the package (its
TypeScript and esbuild, then the WebAssembly component), hands the build
to the running Pane (starting one if none is running), then builds it
again after each save and has Pane reload it, until Ctrl+C. Pane's
development-mode documentation describes what happens when a save does
not build.

`npm run check` reports what Pane would refuse at install, and `npm run
pack` builds the release components and assembles what its users will
download.

[`@pane-app/extension`]: https://www.npmjs.com/package/@pane-app/extension
[`@pane-app/cli`]: https://www.npmjs.com/package/@pane-app/cli
