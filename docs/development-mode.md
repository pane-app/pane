# Development mode: build and reload on save

Added for [#12](https://github.com/pane-app/pane/issues/12) (Rust) and
[#13](https://github.com/pane-app/pane/issues/13) (JavaScript and
TypeScript): US19, US51, US53, T07, T08, G1, G3; contributions, not a claim
that the whole scenario or gate passes. An author edits an extension, saves,
and sees the new behavior while Pane stays open ([ADR 0004](adr/0004-reload-extensions-without-restarting-launcher.md),
Q14). It builds on [reload](../guests/README.md#reloading-a-package-while-pane-stays-open)
(#11) and [pausing](pausing.md) (#16).

## Trying it

The development samples are one command each, "Say hello", which answers
with a `GREETING` constant; each builds in its own folder, as an author's
package would.

**Rust** ([`guests/hello-rust`](../guests/hello-rust)):

1. Build it once, from the repository root:
   `cd guests/hello-rust && cargo build --release --target wasm32-wasip2`.
   (Outside this repository, copy the folder, point `pane-extension`'s `path`
   in `Cargo.toml` at `guests/pane-extension` of a Pane checkout and copy
   `rust-toolchain.toml` beside it.)
2. `cargo run -p pane -- --install guests/hello-rust`, and Enter on
   **Install**.
3. In **Settings › Extensions**, choose **Develop Hello Rust** (the list's
   last rows). The status says which command each save runs.
4. Edit `GREETING` in `guests/hello-rust/src/lib.rs` and save. The status
   shows "Building Hello Rust: cargo build --release --target
   wasm32-wasip2 --message-format=json-render-diagnostics…", then
   "Reloaded Hello Rust"; "Say hello" answers with the new text.
5. Save something that does not compile (`const GREETING: &str = 42;`): the
   status says "Hello Rust did not build: error[E0308]: mismatched types.
   It keeps running its installed code; …", and **Why Hello Rust did not
   build** in Settings › Extensions shows the compiler's output. "Say hello"
   still answers as before. Fix it and save: it is reloaded.
6. **Stop developing Hello Rust** ends it.

**JavaScript and TypeScript** ([`guests/hello-js`](../guests/hello-js),
[`guests/hello-ts`](../guests/hello-ts)) need Node.js 22+ with npm, and
nothing else: no Python, no nightly Rust, no wasi-sdk
([prerequisites](../guests/README.md#writing-a-javascript-or-typescript-command)):
the componentizer is `pane-ext`'s own, linked in (#218).

1. Build it once and hand it to Pane:
   `cargo run -p pane-ext -- dev guests/hello-ts`, and Enter on
   **Install** in Pane's preview: it builds the package here (`npm ci` of
   its locked dependencies, its `tsc`, esbuild, componentization) and Pane
   develops it with that build.
2. Edit `GREETING` in `src/index.ts` and save. A type error
   (`const GREETING: string = 42;`) is shown as "Hello TypeScript did not
   build: src/index.ts(12,7): error TS2322: …".

The `pane-ext` must be one that links the componentizer
(`crates/pane-build`'s `componentizer` feature) and embeds the committed
`runtime.wasm` and `libc.so`
(`tools/componentize-js/wasm-parts`) — one built from a checkout, or the
`@pane-app/cli` npm package (whose JavaScript shim picks the platform
package for the system), which the `cli-packages` workflow builds the same
way.

## What Pane does

Development is turned on per installed, enabled package, from its **Develop
<title>** row, and runs in the background:

1. **Watching.** Pane watches the package's source folder (its identity,
   `local:` and the folder, resolved to its canonical path, as FSEvents
   reports it) with the system's file watcher ([notify](https://docs.rs/notify/8.2.0)
   8.2: inotify on Linux, FSEvents on macOS, `ReadDirectoryChangesW` on
   Windows). It watches the folder itself and each top-level folder that is
   not the build's; a folder created or moved to the top later is watched
   when it appears. These are never saves, at any depth: anything inside a
   `target/`, `node_modules/` or `dist/` folder; `Cargo.lock` (Rust); the
   components `pane.json` names and the top-level folder they are in;
   hidden files and folders (`.git`); and editors' temporary files (vim's
   `4913` probe and `.swp`/`.swo`/`.swx` swap files, `name~` backups,
   JetBrains' `___jb_tmp___` and `___jb_old___`, Emacs's `.#name` and
   `#name#`). Reading a file is not a save, and neither is an event that
   changed nothing Pane can see: each is checked against what Pane last saw
   of that path (from when development started), and a save is a file
   whose modification time or size changed (or, when it was written within
   the clock's resolution of when Pane last looked, its bytes), or a file
   or folder that appeared or went. So a change of metadata alone
   (permissions, a copy by cloning, which FSEvents reports), a folder that
   Windows reports as modified because its entries changed, FSEvents
   telling of a write from before development started, and Pane's own copy
   of the components into the folder after a reload are not saves; when
   the watcher lost events, the whole folder is looked at again.
2. **Building.** After a save, once nothing more is saved for 150 ms (an
   editor's several writes are one save), Pane copies `pane.json` and the
   files of the [helpers](helpers.md) the package ships for this system
   (which no build makes) to a staging folder of the build's own, under
   Pane's data folder
   (`extensions/develop/<hash of the identity>/staging/build-<n>`), and runs
   the package's build in the source folder, one adapter per language, the
   command the guest README documents:

   | Folder has | Build |
   | --- | --- |
   | `Cargo.toml` | `cargo build --release --target wasm32-wasip2 --message-format=json-render-diagnostics`. The component taken is the `.wasm` of that name that cargo's messages report built by this run, wherever the target folder is (a workspace member's, `CARGO_TARGET_DIR`, `build.target-dir`), never an older file where `pane.json` points; if cargo built none of that name, the build fails: "cargo built no <name> this time, …". |
   | `package.json` | Pane's own JavaScript build (`crates/pane-build`): `npm ci --ignore-scripts` of the locked dependencies into a staging copy when the lockfile changed, the package's `tsc` when it has a `tsconfig.json`, `esbuild` around Pane's adapter, then componentization with Pane's componentizer — once for each component `pane.json` names; a failure of the build itself is one line starting `pane-js: error:` |
   | neither | Not developed: "Cannot develop <title>: … has neither Cargo.toml (Rust) nor package.json (JavaScript or TypeScript) …" |

   **Tools.** Cargo is `cargo` on `PATH` (rustup's proxy, so the folder's
   `rust-toolchain.toml` applies), else `~/.cargo/bin/cargo`
   (`%USERPROFILE%\.cargo\bin\cargo.exe` on Windows); Node.js and npm are
   found on `PATH` (`npm` is `npm.cmd` on Windows). The componentizer of a
   JavaScript or TypeScript build is Pane's own: `pane-ext` and Pane's own
   development tests link it in (with the committed `runtime.wasm` and
   `libc.so` embedded); the app uses the componentizer of the package's own
   installed `@pane-app/cli` platform package
   (`node_modules/@pane-app/cli-<target>`, which `npm install` provides —
   a package without one is explained: run npm install), or the folder
   `PANE_COMPONENTIZER` names (a Pane checkout's
   `target/guests/componentizer`, which `cargo xtask guests` builds).
   Without one, "Cannot develop <title>: …" names where it looked.

   **Environment.** A build runs with Pane's environment, so the author's
   cargo configuration applies (`CARGO_HOME`, `CARGO_TARGET_DIR`, registry
   tokens, `CARGO_HTTP_*`, `CARGO_NET_OFFLINE`, proxies). Only what
   `cargo run` or `cargo test` set for Pane itself is removed: `CARGO`,
   `CARGO_MANIFEST_DIR`, `CARGO_MANIFEST_PATH`, `CARGO_PKG_*`,
   `CARGO_BIN_NAME`, `CARGO_CRATE_NAME`, `CARGO_PRIMARY_PACKAGE`,
   `CARGO_BIN_EXE_*`, `CARGO_TARGET_TMPDIR`, `CARGO_RUSTC_CURRENT_DIR`,
   `OUT_DIR`, `RUSTUP_TOOLCHAIN`, `RUSTC` and `RUSTDOC`, and the
   `LD_LIBRARY_PATH`/`DYLD_*` entries cargo added for Pane's target and
   toolchain folders. (The JavaScript build's spawned steps — `npm`, `tsc`,
   `esbuild`, a componentizer binary — run with that same environment.)

   **What a save runs.** Once development is on, any write to the folder
   (an editor's autosave, `git pull`, a sync client) runs the build,
   including a Rust package's `build.rs` and procedural macros, with the
   author's rights. That is what the author asked for by developing that
   package, for this session only; nothing is built for a package that is
   not developed.

   The status shows "Building <title>: <command>…". The build's standard
   output and error are read line by line (each line whole); the last 2,000
   lines (at most 256 KiB) are kept in memory, and all of it is written to
   `extensions/develop/<hash>/build.log` in Pane's data folder.
3. **A build that fails** replaces nothing: the package keeps running its
   installed code. The status says "<title> did not build: <first error>.
   It keeps running its installed code; the diagnostics are under "Why
   <title> did not build" in Settings › Extensions." The first error is the
   first line that rustc or cargo (`error:`, `error[E0308]:`), TypeScript
   (`error TS2322`) or Pane's JavaScript build (`pane-js: error:`) report
   as one; a
   line such as `Compiling thiserror` is not; without one, why the build
   failed (the command and its exit code). The log is kept as
   `failed-build.log`, and **Why <title> did not build** opens the command,
   the folder, the log's path and the last 60 lines of the output, with
   **Build <title> again**. Pane's standard error gets one line: the first
   error and the log's path. The row goes once a build succeeds.
4. **A build that succeeds** is reloaded from its staging folder exactly as
   **Reload <title>** reloads the source folder: checked as an install
   checks it (a component that does not pass is "not reloaded", the code
   keeps running), then replacing the managed copy and starting each
   available command. A start that fails pauses the package with Retry and
   diagnostics, and **the earlier code is not restored** (Q31,
   [pausing](pausing.md)); the next save that builds reloads it, which ends
   the pause. Settings are kept; nothing live is carried over, and a
   running helper of the package is stopped before its copy is replaced,
   as a Reload does. The
   components are then copied to where `pane.json` names them in the source
   folder, so a later **Reload** reloads the same build. If the package is
   being changed otherwise when the build ends (a **Reload**, an update),
   the build waits for that to end; a save meanwhile makes it obsolete.
5. **Saving during a build** makes that build obsolete: it runs to its end
   (it is not killed), but is never reloaded, and the folder is built
   again. Builds of one package run one at a time, and each reload ends
   before the next build starts, so an older build never replaces a newer
   one. What an obsolete build left where `pane.json` names the components
   (cargo's `target`) is replaced with the installed components, so a later
   **Reload** cannot pick it up. After three obsolete builds in a row the
   status says "<title> was not reloaded: its sources kept changing during
   3 builds in a row. Save again to build it.", and Pane waits for the next
   save.
6. **The status line.** A development status is shown on Manage
   extensions, a build's details and root search with nothing typed. On
   another screen (an open command, a form, a query's results) it does not
   replace what that screen says; the newest is shown when the user returns
   to Settings › Extensions or root search.
7. **The extension log.** While the package is developed, what its code
   writes to standard output and standard error, and Pane's own messages
   about it (each status above, its crashes with their backtraces, calls
   that stopped responding, pauses), are kept as its extension log: the
   most recent 5,000 lines in memory, which the launcher hands to whoever
   follows the log, and every line in `extensions/develop/<hash>/extension.log`
   beside the build's log, rotated at 5 MiB into `extension.log.1`. The
   file starts afresh with each development session and is kept when it
   ends. A package not developed keeps only its most recent 500 lines in
   memory, never on disk. Lines are cut at 4 KiB, and a package writing
   more than 1,000 lines in a second loses the rest of that second, with a
   note of how many ([printing and logging](../guests/README.md#printing-and-logging)).
8. **Logs for <title>.** The package's Actions menu in Settings ›
   Extensions offers **Logs for <title>** beside Stop developing, and so do
   a failed build's details; it opens the log in the launcher window, which
   comes forward. Each line shows its local time, its level (errors and
   warnings in their colours, debug lines muted) and who wrote it: the
   extension, or Pane, whose lines carry an accent mark. The list follows
   new lines while it is at its end, the newest selected; scrolling up or
   moving the selection up stops following, and End, the last line or
   scrolling back to the end follows again. Enter (or Ctrl+C) copies the
   selected line and Ctrl+Shift+C every line, as the log file writes them;
   Ctrl+L clears the lines Pane keeps, not the file; Ctrl+O opens the log
   file (Command instead of Ctrl on macOS).

Only the developed package is built and reloaded; Pane and every other
package keep running.

## When development ends

When the author chooses **Stop developing <title>**, disables or
uninstalls the package, or Pane quits (and when the launcher is dropped, as
in tests). The watcher is dropped, and a running build's processes are
killed before that action returns; a build that ends after it is dropped
without a word. Enabling the package again does not develop it again.

- **macOS and Linux:** the build runs in a process group of its own, killed
  with `SIGKILL`. After its command exits, what is left of the group is
  killed too, and its output is read for at most 2 more seconds, so a
  daemon it started that holds the pipes cannot hold Pane. On Linux the
  command also gets `SIGKILL` if Pane dies without quitting
  (`PR_SET_PDEATHSIG`); macOS has no such signal, so there a build
  outlives a Pane that is killed or crashes.
- **Windows:** the build runs in a Job Object that kills its processes when
  closed (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`), so they die when
  development stops and when Pane ends however it ends; it also runs in a
  new process group (`CREATE_NEW_PROCESS_GROUP`), without a console window.
  A process the command starts in the instant before it is assigned to the
  job escapes it.

## Starting a package: `pane-ext new`

`pane-ext new [folder]` ([#221](https://github.com/pane-app/pane/issues/221),
[ADR 0047](adr/0047-extensions-are-built-from-the-app-first-with-pane-ext-beside-it.md))
writes a fresh extension package, from the templates the ADR settles —
`list`, `detail`, `form` and `no-view`, in TypeScript and in Rust — kept
in the repository as the files `pane-core` embeds
([`crates/pane-core/templates`](../crates/pane-core/templates)), so the
CLI and the app's Create Extension command write the same package.
`npm create @pane-app` runs it too, through the
[`@pane-app/create`](../packages/create) package, so an author starts with
Node.js and npm alone:

```
npm create @pane-app notes -- --language typescript --template list
cd notes
npm install
npm run dev
```

Every choice has a flag — `--name` (the extension's title, from which its
package name and command id come), `--language` (`rust` or `typescript`)
and `--template` — and in an interactive terminal the missing ones are
asked for, each with a default; a script that passes them all is never
asked. An existing folder is written into only while empty: an author's
files are never overwritten. A template's folder holds what a package
needs: `pane.json` with a `"$schema"` an editor checks (Pane ignores
fields it does not know), the command's source, a README, a placeholder
512×512 icon, `.gitignore`, and either a `package.json` (naming
`@pane-app/extension` and `@pane-app/cli` from npm, whose `dev`, `check`
and `pack` scripts run `pane-ext`) or a `Cargo.toml` (naming the
`pane-extension` crate, with a stable toolchain file), plus the
formatter's, linter's and type checker's configuration.

`pane-ext new command <folder>` adds a command to a package that already
exists: an entry in its `pane.json` (served by the component its other
commands are, as one component serves every command of a package), a
source file, and the entry file's dispatch arm, added at a marker the
templates write, so the package still builds and the command runs without
the author touching anything.

## Starting in the app: Create Extension and Import Extension

**Create Extension…** ([#222](https://github.com/pane-app/pane/issues/222),
[ADR 0047](adr/0047-extensions-are-built-from-the-app-first-with-pane-ext-beside-it.md))
is the root-search row an author starts from in the app, beside the
install rows. It asks for the parent folder (the system's folder picker),
then a form: the extension's name, its language (TypeScript or Rust) and
its template (list, detail, form or no-view). Submitting it:

1. writes `<parent>/<package name>` from the same templates `pane-ext new`
   writes (an existing non-empty folder is refused: an author's files are
   never overwritten);
2. for TypeScript, runs `npm install` in the folder when npm is found —
   it writes the lockfile the build's `npm ci` runs from and the
   `node_modules` the package's own scripts use — and a failure stops
   nothing: the build that follows explains what is missing, as it does
   when npm install was skipped;
3. builds the package once with the same builder development mode uses,
   copying the built components into the folder, as `pane-ext dev`'s
   first run of a new folder does;
4. shows Pane's ordinary install preview, which the author confirms with
   **Install**, and then develops the package: each save in the folder
   builds and reloads it, as below. The status line says each step —
   "Installing …'s dependencies", "Building …" — while it runs.

Building still needs the author's tools, whatever wrote the folder: Node
and npm for TypeScript, or rustup with the `wasm32-wasip2` target for
Rust. A missing one is named, with where to get it (Node.js from
<https://nodejs.org>; rustup from <https://rustup.rs> and `rustup target
add wasm32-wasip2`); a build failure is the build's own first error with
the folder and the whole output's log, and the folder is kept for
importing once it builds.

**Import Extension…** asks for the folder of a package that already
exists and shows the same install preview of it — a folder without
`pane.json`, or a source-only package whose components are not built, is
explained by Pane's own messages, since the picker cannot look for them —
and develops it once installed, as Manage extensions' local install with
development does. Leaving either flow's preview without installing drops
what it would have developed: not installing the package is the author's
answer.

## From the terminal: `pane-ext dev`

`pane-ext dev [folder]` ([#217](https://github.com/pane-app/pane/issues/217),
[ADR 0047](adr/0047-extensions-are-built-from-the-app-first-with-pane-ext-beside-it.md))
develops the package in `folder` (the current folder by default) from the
author's terminal. It runs the same session as Pane's own development (the
`pane-build` crate's), so the build command, the obsolete builds and the
copying of components into the source folder are the same; the difference
is where the builds run and print:

1. It reaches the running Pane over the **local channel**, a per-user
   endpoint only the same user can open: the named pipe
   `\\.\pipe\pane-<the user's SID>` on Windows, whose protected security
   descriptor grants this user alone and which refuses remote clients; the
   socket `channel` in `$XDG_RUNTIME_DIR/pane`, else in `pane-<uid>` in the
   temporary folder, on macOS and Linux, in a folder of mode 0700 (Pane
   does not listen in one open to others), itself mode 0600, closing
   connections from other users unanswered; `pane-ext` does not connect
   through such a folder that is not the user's own. `PANE_CHANNEL` names
   another endpoint, to both.
2. While its first build runs (step 3), if no Pane answers, it starts one:
   the program `PANE_APP` names, else
   `pane` beside `pane-ext`, else where Pane's packages install it
   (`%LOCALAPPDATA%\Pane\pane.exe`, `Pane.app` in `/Applications` or
   `~/Applications`, `~/.local/bin/pane`), else `pane` on the search path,
   and waits up to a minute for it to listen. Found nowhere, it says where
   it looked and exits, stopping the build.
3. It builds the package in the terminal, printing what the build prints,
   cargo's errors included, and stages it in a folder of its own in the
   user's cache folder (`pane-ext/<hash of the folder>`).
4. It hands the first build that succeeds to Pane. A folder Pane has not
   installed is first shown in Pane's ordinary install preview, with this
   build (its components are copied into the folder), and the author
   chooses **Install** there; leaving the preview refuses the build, and
   `pane-ext` exits. Pane then develops the package without watching or
   building it: the rows, the status line, **Why <title> did not build**,
   the extension log and its file are as for its own development, the
   status saying "Developing <title> with pane-ext". An installed folder is
   developed at once, from this build; a development already going on,
   Pane's own or another `pane-ext`'s, ends first.
5. Each save builds the package again in the terminal; Pane reloads each
   build that succeeds, and a build that fails leaves Pane running the
   working code, with the failure shown in Pane as its own are. **Build
   <title> again** asks `pane-ext` to build.
6. Pane's messages about the package and what the package prints, its
   extension log, are printed in the terminal as they come ("Pane: Reloaded
   Hello Rust", "info [hello] …").
7. Ctrl+C, or the connection closing, stops the development as **Stop
   developing** does (the package stays installed). Stopping it in Pane,
   disabling or uninstalling the package, or quitting Pane ends
   `pane-ext dev`.

The channel's requests are JSON, one a line, each naming the channel's
version (1), as `pane_core::local_channel` documents: `subscribe`,
`develop` (a folder and a staged build), `building` and `failed` (later
builds) and `stop`. Pane answers with `previewing`, `developing`,
`refused`, `build`, `log` and `ended` events. A request of another version
is refused, saying so.

Rust packages work end to end, and so do JavaScript and TypeScript ones:
`pane-ext dev` builds them with pane-build's JavaScript build and the
componentizer it links (#218). An installed Pane builds them with the
componentizer of the package's own `@pane-app/cli` platform package (which
`npm install` provides), and a package without one is explained.

## Local and published copies

Development applies to one installation, identified by its source folder
([identity](../guests/README.md#packaging-and-installing-a-local-extension)).
Another installed copy of the same package, from another folder (the
"published" copy, until npm and Git sources exist), is never built,
reloaded, disabled or swapped for it (Q30): both stay listed, each with its
own commands, settings and code, and a published copy without sources is
explained ("Cannot develop <title>: …") rather than developed.

## Choices to confirm (provisional)

These are implementation choices of #12/#13, not user decisions:

- Development is turned on per package in Settings › Extensions and is **not
  recorded**: it ends when Pane quits, and starting it does not build at
  once (the next save does).
- The build is chosen by the folder's `Cargo.toml` or `package.json`, with
  the documented commands; a package cannot declare a build of its own.
- A save during a build lets that build finish and builds again, rather than
  killing it; after three obsolete builds in a row Pane waits for the next
  save rather than reloading the last one that built.
- A build failure is shown in the status line and a details row, not as a
  separate notification; the details show the last 60 lines and the log's
  path.
- **Build <title> again** on the details screen reruns the build without a
  save.
- A development status waits on screens other than Settings › Extensions, a
  build's details and an empty root search.
- The JavaScript build runs `tsc` in the staged copy of the package, so a
  type error names the file as the package has it (`src/index.ts(12,7)`).
- Reloading on save starts every available command, as a manual reload does
  (the provisional exception to lazy activation in
  [current decisions](current-decisions.md)).
- The rows are near the end of Settings › Extensions, one per enabled
  package, after the hotkey rows and before retained data.

- `pane-ext dev` (#217): the request set above; the endpoint's locations;
  where `pane-ext` looks for a Pane to start, and the minute it waits; that
  Pane waits up to 30 seconds for its window to show the install preview it
  asked for, and then for as long as the author takes to answer it; that
  the first `pane-ext dev` of a folder not installed copies the build's
  components into the folder, so that the preview and the install are of
  that build; that a second `pane-ext dev` of the same package takes the
  development over, and the first is then refused rather than taking it
  back; and that `pane-ext` reaches or starts Pane while its first build
  runs, so that a missing Pane is reported at once.

## Checks

- `pane-core`'s unit tests on `templates` check every scaffold's file set,
  its manifest (through Pane's own reading of it), its placeholder icon
  and `pane-ext new command`'s edits; `pane-ext`'s tests on
  [`new`](../crates/pane-ext/tests/new.rs) and
  [`create`](../crates/pane-ext/tests/create.rs) run the commands and the
  `@pane-app/create` package as processes, and
  [`templates`](../crates/pane-ext/tests/templates.rs) builds and
  installs every template in both languages through the build and
  launcher above (#221).
- [`crates/pane-core/tests/create.rs`](../crates/pane-core/tests/create.rs)
  drives Create Extension and Import Extension through the launcher with
  a stand-in build that stages the guest the scaffolded manifest names
  (#222): the authoring rows and what they ask the window for; the form's
  fields and a name that names no package marked on its field; the folder
  written (in both languages) and built once, its component copied in;
  the preview that follows and the package developed once installed, in
  both flows and for an installed folder again; a folder that is not
  empty refused, a build failure shown with the folder and its log, a
  Pane without a builder naming that it cannot build, a folder that is
  no package explained by the preview, and leaving the preview dropping
  what would have been developed.
- [`crates/pane-core/tests/develop.rs`](../crates/pane-core/tests/develop.rs)
  drives development through the launcher with the system's file watcher
  and a stand-in build that stages a real guest, waiting on what the
  development reports (and on a second developed package's build, to know
  that a save was not acted on) rather than on fixed pauses: a save
  reloads only that package (another keeps its code); a build that fails
  keeps the code and shows its first error, output and log, which Build
  again reruns, and a fix reloads it; a build that fails to start is
  paused with Retry and not rolled back, and the next save recovers it; a
  save during a build makes it obsolete (only the newer build is reloaded
  and copied to the source folder), and an obsolete build's leftovers are
  replaced with the installed code for a later Reload; three obsolete
  builds in a row wait for the next save; a build that ends during a
  Reload waits for it, and one that ends after development stopped is
  dropped without a word; stopping, disabling, uninstalling and dropping
  the launcher each stop the running build and the watcher; a status does
  not replace an open command's answer and is shown back at root search; a
  folder moved into the source folder is watched, and editors' temporary
  files are not saves; reading the sources or changing only their
  metadata (a file's or a folder's) is not a save; a published copy keeps its identity and code; the
  rows in Settings › Extensions; the window is told of each change.
- [`crates/pane-core/tests/develop_builds.rs`](../crates/pane-core/tests/develop_builds.rs)
  runs the real builds on copies of the samples: an edit is built and
  reloaded, a compile or type error keeps the code and shows the compiler's
  first error and output, with the log, a fix reloads it; a Rust package
  with its own `build.target-dir` reloads what cargo built this time, not
  the older file where `pane.json` points, and a component cargo did not
  build is refused. The Rust tests run in `cargo xtask ci`; the JavaScript
  and TypeScript ones run un-gated with Node.js and npm alone (the
  componentizer pane-core's dev-dependencies links in, #218), and one test
  drives the package-installed componentizer an installed Pane spawns.
- Unit tests in Pane's core's [`develop.rs`](../crates/pane-core/src/develop.rs):
  the adapters' commands (paths with spaces quoted) and ignored paths, a
  folder without a known build or tool, and staging `pane.json` with this
  system's helper files.
- Unit tests in the shared build crate's
  [`build.rs`](../crates/pane-build/src/build.rs) (`pane-build`, which
  holds the builds, the session that runs them after each save and the
  process trees; `pane-ext` builds with it too, ADR 0047): a package built
  once returns its staged components or why it failed, the first error
  (not `thiserror`), cargo's artifact messages, which variables are
  removed, the bounded output and its log, whole lines from both pipes,
  and (Unix) stopping a command kills what it started at once, a command
  whose child keeps the pipes open still returns, and one whose daemon
  (outside the group) keeps them open returns after the 2-second drain.
- Unit tests in [`sources.rs`](../crates/pane-build/src/sources.rs):
  what was already there, a read and a change of permissions are not
  saves; other bytes of the same size written at once are, once; a file or
  folder appearing or going is, and a folder that appears is watched;
  what Pane wrote is not; a rescan finds what changed.
- [`crates/pane-core/tests/helpers.rs`](../crates/pane-core/tests/helpers.rs):
  Pane's copy of the component into the folder after a reload starts no
  build, even for a build that ignores nothing.
- [`crates/pane/tests/develop.rs`](../crates/pane/tests/develop.rs): the
  window redraws by itself when a background build fails and when the fix
  is reloaded, and renders the diagnostics.
- [`crates/pane/tests/create.rs`](../crates/pane/tests/create.rs): Create
  Extension and Import Extension in the window with real key events
  (#222): the rows, the form filled with the keyboard (a choice picked
  with its own keys), the created package built and previewed, installed
  and developed, and the imported folder developed; a name that names no
  package marked on its field.
- [`crates/pane-core/tests/local_channel.rs`](../crates/pane-core/tests/local_channel.rs)
  speaks the local channel as `pane-ext` does, against a launcher listening
  on an endpoint of its own, with guests staged as builds: a folder not
  installed is previewed with the build and developed once installed (its
  log, Pane's messages, a failure kept as Pane's own, a later build
  reloaded), and closing the connection stops the development; a preview
  left refuses the build; an installed folder is developed at once, and
  stopping it in Pane ends the connection's; an unknown folder is refused.
  Unit tests in [`local_channel.rs`](../crates/pane-core/src/local_channel.rs):
  the requests' and events' JSON, other versions refused, a second Pane
  cannot listen on the endpoint, and the endpoint is open to this user only
  (the socket's and its folder's modes, or the pipe's DACL).
- [`crates/pane-ext/tests/dev.rs`](../crates/pane-ext/tests/dev.rs) runs
  `pane-ext dev` on a copy of `guests/hello-rust` against a launcher
  listening as the running Pane: the build's output, the install preview,
  Pane's messages and the package's log line in the terminal; a compile
  error printed there while Pane keeps the working code; a fix reloaded;
  the development stopped when `pane-ext` is killed. With no Pane listening
  and none to start, it says where it looked. Its unit
  tests: waiting for a Pane that starts listening, and giving up on one
  that does not.
- The native smokes' development phase (screenshots 110 to 136; see the
  [platform notes](platforms/linux.md#development-mode-12-13)).

## Limits

- A build has no timeout: one that hangs holds the package's development
  until it ends or development stops.
- On macOS, a Pane that is killed or crashes leaves a running build to
  finish on its own (see above); a daemon a build starts in a session of
  its own (`setsid`) is not killed anywhere but Windows.
- An installed Pane without a checkout builds a JavaScript or TypeScript
  package on save with the componentizer of the package's own
  `@pane-app/cli` platform package (`node_modules/@pane-app/cli-<target>`,
  which `npm install` provides; `#219` will publish those). A package
  without one is explained; a Pane built from a checkout links the
  componentizer itself (#218), as `pane-ext` does.
- On macOS and Linux, Ctrl+C in `pane-ext dev`'s terminal ends `pane-ext`
  but not a build it is running, which is in a process group of its own:
  the build runs to its end, and Pane is told nothing of it. On Windows the
  console's Ctrl+C reaches the build too.
- That another user cannot open the endpoint is checked in CI through its
  permissions (the socket's and its folder's modes, the pipe's DACL), not
  by connecting as another user. On Windows, `pane-ext` does not check who
  created the pipe it connects to: another user's pipe of that name,
  created while Pane is not running, would receive its requests (the
  folder and the staged build's path).
- Native evidence is from Linux (X11) only; the macOS and Windows smokes run
  the same phase, not yet run there. The platform code (process groups, the
  Job Object, the watcher) was compile- and lint-checked for Windows and
  macOS targets from Linux.
