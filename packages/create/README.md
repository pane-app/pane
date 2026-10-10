# @pane-app/create

`npm create @pane-app` starts a Pane extension package: it runs
`pane-ext new` with the arguments it was given, in the terminal, with all
of pane-ext's prompts and defaults.

```
npm create @pane-app notes -- --language typescript --template list
cd notes
npm install
npm run dev
```

`--language` picks `typescript` (the default) or `rust`; `--template`
picks `list` (the default), `detail`, `form` or `no-view`. Without
arguments, pane-ext asks for what it needs: an extension title, a
language and a template, then writes the package folder from the name
alone.

## Where pane-ext comes from

This package depends on [`@pane-app/cli`], whose per-platform packages put
the `pane-ext` binary in `node_modules/@pane-app/cli-<target>`, the same
place Pane's own build looks for the componentizer, so npm's one-step
`create` installs it and this package finds it. A `pane-ext` on the PATH
(a globally installed CLI) serves too. With neither, this package says
how to get one.

Publishing this package is a person's step, never CI's.

[`@pane-app/cli`]: https://www.npmjs.com/package/@pane-app/cli
