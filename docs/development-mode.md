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
[`guests/hello-ts`](../guests/hello-ts)) need the JS toolchain
([prerequisites](../guests/README.md#writing-a-javascript-or-typescript-command)):

1. Build once: `python3 tools/componentize-js/pane_js.py build
   guests/hello-ts guests/hello-ts/dist/hello_ts.wasm`.
2. Install `guests/hello-ts`, choose **Develop Hello TypeScript**, edit
   `GREETING` in `src/index.ts` and save. A type error
   (`const GREETING: string = 42;`) is shown as "Hello TypeScript did not
   build: src/index.ts(12,7): error TS2322: …".

The Pane window must be one built from a checkout (`cargo run -p pane`),
which knows where `tools/componentize-js/pane_js.py` is; otherwise set
`PANE_COMPONENTIZE_JS` to that file.

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
   | `package.json` | `python3 <pane_js.py> build <folder> <staging>/<component>` for each component `pane.json` names; a failure of `pane_js.py` itself is one line starting `pane-js: error:` |
   | neither | Not developed: "Cannot develop <title>: … has neither Cargo.toml (Rust) nor package.json (JavaScript or TypeScript) …" |

   **Tools.** Cargo is `cargo` on `PATH` (rustup's proxy, so the folder's
   `rust-toolchain.toml` applies), else `~/.cargo/bin/cargo`
   (`%USERPROFILE%\.cargo\bin\cargo.exe` on Windows). Python is
   `PANE_PYTHON`, else `python3` on `PATH`, else on Windows `python`,
   skipping the Microsoft Store stub in `WindowsApps`. Without one, "Cannot
   develop <title>: Pane found no cargo … (it looked for …)" names where it
   looked.

   **Environment.** A build runs with Pane's environment, so the author's
   cargo configuration applies (`CARGO_HOME`, `CARGO_TARGET_DIR`, registry
   tokens, `CARGO_HTTP_*`, `CARGO_NET_OFFLINE`, proxies). Only what
   `cargo run` or `cargo test` set for Pane itself is removed: `CARGO`,
   `CARGO_MANIFEST_DIR`, `CARGO_MANIFEST_PATH`, `CARGO_PKG_*`,
   `CARGO_BIN_NAME`, `CARGO_CRATE_NAME`, `CARGO_PRIMARY_PACKAGE`,
   `CARGO_BIN_EXE_*`, `CARGO_TARGET_TMPDIR`, `CARGO_RUSTC_CURRENT_DIR`,
   `OUT_DIR`, `RUSTUP_TOOLCHAIN`, `RUSTC` and `RUSTDOC`, and the
   `LD_LIBRARY_PATH`/`DYLD_*` entries cargo added for Pane's target and
   toolchain folders. (`pane_js.py` strips every `CARGO_*` but
   `CARGO_HOME` for the toolchain it builds itself.)

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
   (`error TS2322`) or `pane_js.py` (`pane-js: error:`) report as one; a
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
- `pane_js.py` runs `tsc` in the staged copy of the package, so a type
  error names the file as the package has it (`src/index.ts(12,7)`).
- Reloading on save starts every available command, as a manual reload does
  (the provisional exception to lazy activation in
  [current decisions](current-decisions.md)).
- The rows are near the end of Settings › Extensions, one per enabled
  package, after the hotkey rows and before retained data.

## Checks

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
  and TypeScript ones need the JS toolchain and run with
  `PANE_TEST_JS_BUILDS=1` (CI's JS/TS job sets it).
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
- The native smokes' development phase (screenshots 110 to 136; see the
  [platform notes](platforms/linux.md#development-mode-12-13)).

## Limits

- A build has no timeout: one that hangs holds the package's development
  until it ends or development stops.
- On macOS, a Pane that is killed or crashes leaves a running build to
  finish on its own (see above); a daemon a build starts in a session of
  its own (`setsid`) is not killed anywhere but Windows.
- The JS/TS build needs a Pane checkout's `pane_js.py` (and its toolchain);
  an installed Pane without a checkout cannot build JS/TS on save.
- Native evidence is from Linux (X11) only; the macOS and Windows smokes run
  the same phase, not yet run there. The platform code (process groups, the
  Job Object, the watcher) was compile- and lint-checked for Windows and
  macOS targets from Linux.
