# Greeter from Git

Pane's sample of an extension distributed as a Git repository: a Rust
command, "Say hello", and the operation `greet`, whose answers name the Git
repository.

This folder is the package's **source**: `pane.json` names the component
`dist/git_greeter.wasm`, which is not here until it is built. A revision
holding only the source is a *source-only* revision: Pane explains it and
installs nothing, since it never builds a package or runs anything from a
repository.

A **release** revision adds the built component:

1. Build it: `cargo build --release --target wasm32-wasip2` (here, `cargo
   xtask guests` from Pane's checkout builds it with the other samples).
2. Copy `target/wasm32-wasip2/release/git_greeter.wasm` to
   `dist/git_greeter.wasm` beside `pane.json`.
3. Commit `dist/` on a release branch and tag that commit, keeping your
   development branch free of build outputs if you like: for example `git
   switch -c release`, `git add -f dist`, `git commit -m "Release 0.1.0"`,
   `git tag v0.1.0`, then push the branch and the tag.

Users then install `https://<host>/<owner>/<repo>@v0.1.0` (pinned to the
tag) or `…@release` (tracking the branch) from **Install extension from
Git…** in Pane. See [docs/git.md](../../../docs/git.md).
