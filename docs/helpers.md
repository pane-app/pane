# Native helpers

Added for [#15](https://github.com/hoangvu12/pane/issues/15) (US40, US41,
US42, US64; T09, T22; contributions to G2 and G3, not claims that they
pass). A command can run a **native helper**
([glossary](../CONTEXT.md)): a prebuilt program its package ships for each
operating system and processor it supports, for functionality a WASI 0.3
guest cannot reach itself ([ADR 0014](adr/0014-optional-native-extension-helpers.md)).
Pane runs the file for the system it runs on, passes its input and output,
explains a helper it cannot run, and ends the helper's process when the
command cancels the run and when the package is disabled, reloaded,
updated, paused or uninstalled. The command itself stays a WASI 0.3
component; Pane never compiles a helper.

## The contract

- **Host import** `pane:extension/helpers` ([`wit/helpers.wit`](../wit/helpers.wit)),
  in the world Pane hosts, `extension-with-helpers`: one function,
  `run(helper, args, input) -> result<string, helper-error>`. Pane starts the
  package's helper `helper` with `args`, writes `input` to its standard input
  and closes it, and answers with what it wrote to its standard output once
  it exits with success. Rust commands call it as
  `pane_guest::helpers::run` ([pane-guest](../guests/pane-guest/src/lib.rs));
  JavaScript and TypeScript commands import `run` from
  `pane:extension/helpers@0.1.0` ([declarations](../guests/js/helpers.d.ts)),
  whose promise rejects with the error as its `payload`.
- **Package declaration**, under `helpers` in `pane.json`: each helper's
  `id` and its file for each **target** it is built for, `<os>-<arch>` with
  `windows`, `macos` or `linux` and `x86_64` or `aarch64`:

  ```json
  "helpers": [
    {
      "id": "echo",
      "targets": {
        "linux-x86_64": "helpers/linux-x86_64/pane-echo",
        "macos-aarch64": "helpers/macos-aarch64/pane-echo",
        "windows-x86_64": "helpers/windows-x86_64/pane-echo.exe"
      }
    }
  ]
  ```

  Files are relative paths inside the package folder. A helper `id` is
  lowercase ASCII letters, digits and dashes, starting with a letter or
  digit, at most 64 characters. A Windows target's file must end in `.exe`:
  a `.cmd` or `.bat` file (which Windows runs through its command
  interpreter) or a file without the extension makes the manifest invalid,
  like a target Pane does not know, a path outside the package, a helper
  without targets, a repeated id or an id outside that pattern.
- **Errors** carry a kind and a message for people:

  | Kind | When |
  | --- | --- |
  | `not-found` | The package's `pane.json` declares no helper by that name ("Helper sample declares no helper `absent` in its pane.json; it declares `echo`"). |
  | `unavailable` | No file for this target ("Not available on Linux arm64: helper `echo` is built only for Linux x86-64, macOS arm64 and Windows x86-64"), the file is missing or is a program for another system, or the system would not start it. |
  | `failed` | The helper exited unsuccessfully ("helper `echo` failed (exit code 3): pane-echo was asked to fail", with the last 2 KB of its standard error), or its output is not UTF-8 text. |
  | `refused` | The command's code was stopped (disabled, reloaded, updated, paused, uninstalled), the command is built into Pane rather than an installed package's, the input (1 MiB), arguments (64, 64 KiB) or output (1 MiB) are over Pane's limits, or Pane stopped the run. |

## Installing and choosing the file

- **Install and update** check this system's file, where the package ships
  one: it must be there, and its header must be a program for this target
  (64-bit little-endian ELF on Linux, Mach-O, including universal, on
  macOS, PE on Windows, each with the target's processor; a universal
  Mach-O header is told apart from a Java class file by its architecture
  count). It must be a regular file: not a symbolic link, and inside the
  package folder once links in its path are resolved. A `#!` script is
  refused on every system: a helper is a native program. Otherwise the
  package is refused: "Not ready to run: the package ships helper `echo`
  for Linux x86-64, but its file helpers/linux-x86_64/pane-echo is a program
  for Windows x86-64, not Linux x86-64", "... is a script; a helper must be
  a native program built for Linux x86-64", "... is a symbolic link; a
  helper must be a regular file in the package", or "... is missing".
- A package that ships a helper only for **other targets** installs: its
  other commands and items work, and running the helper explains which
  targets it is built for.
- Only **this system's file** is copied into the managed copy, as a
  regular file, with mode exactly `rwxr-xr-x` (0755) on macOS and Linux (a
  package fetched as an archive may have lost the permission, and set-id
  or world-writable bits are dropped). Other targets' files are not copied.
- The **package preview** lists each helper's targets, marking this
  system's: "Helpers: echo for Linux x86-64 (this system), macOS arm64 and
  Windows x86-64", or "... (none for this system)".
- Running re-checks the file in the managed copy the same way, so a file
  replaced or removed after installing is explained, not started. Pane
  starts the exact checked file by its absolute path, with its extension,
  so the system never searches for or completes another program.
- Pane compiles nothing on the user's machine: the package author builds a
  helper for each target they support and puts the files in the package.

## Running and stopping

A helper's process belongs to Pane. Its standard input, output and error
are unnamed scratch files Pane creates for the run (removed from the folder
at once on macOS and Linux, deleted on close on Windows, readable only by
the user): the input is written before it starts, and the output is read
once it has been reaped, so no thread waits on a pipe and a process the
helper started that keeps the output open blocks nothing. It runs in its
own folder of the installed package, with Pane's environment, and on
Windows without a console window. One supervising thread per run owns the
process: it ends it (`kill`: SIGKILL on macOS and Linux, `TerminateProcess`
on Windows) and reaps it when the first of these happens. Nothing that
stops a helper waits on the runtime thread: it asks the supervising
thread, which reaps; stopping several helpers asks each first, then waits
for all.

| Event | How it reaches the helper |
| --- | --- |
| The command **cancels** the run: it drops the call's future, for example when a timer wins a race with it | Wasmtime cancels the host task (`subtask.cancel`) and drops its future; dropping ends the process. |
| The **Pane call** that started it returns while it still runs | The runtime ends the helpers the instance started when the call ends: a helper runs no longer than the call that started it. |
| The package is **disabled, reloaded, updated, paused or uninstalled** | Its [generation](generations.md) ends, and runs its undo list, on which every run is recorded while it runs: that asks the supervising thread to end the process at once. The supervising thread also checks the generation itself, every 10 ms, so this holds even while the runtime thread is busy in another guest that does not yield; the call is also stopped and its instance dropped as for any call. |
| The guest **instance** goes (it crashed, was forgotten, the runtime stopped) | Dropping the instance's state ends the helpers it started. |
| The **runtime thread crashes** (#17) | Unwinding drops its instances, which ask their helpers to end; then the crashed thread ends and reaps every helper still running, before Pane restarts the runtime or reports it stopped ([runtime crashes](pausing.md#when-the-extension-runtime-itself-crashes)). |
| **Pane quits** | The app's quit handler (`Runtime::quit`) ends every helper and waits for each to be reaped, then stops the runtime thread, dropping every call still waiting; from then on no helper starts ("Pane is quitting; helper `echo` does not start"). Dropping the last runtime handle does the same. |

A helper that writes more than 1 MiB of output is ended too. Saved data is
untouched: what the command saved before the helper was stopped is kept
("started" in the sample), and nothing after its `await` runs.

## Limits

- **Descendants are not contained.** Pane ends the helper's own process,
  not processes it starts: those are the helper's to end. A detached or
  daemonized descendant keeps running on every system; what it writes to
  the output after the helper exits is not read. Process groups, Windows
  job objects and PR_SET_PDEATHSIG are not used.
- **Pane ended from outside** (a signal such as SIGTERM or SIGKILL, a
  crash, Task Manager's End task) ends no helper: a running helper keeps
  going until it exits by itself (its input is closed and its output goes
  nowhere). Quitting Pane (closing its window) ends them; the smokes end
  Pane with a signal only once no helper runs, and one phase quits Pane
  while a helper runs.
- **Update while a helper runs:** once the new copy is in place, the old
  copy's generation ends and its helpers are ended and reaped before its
  folder is removed, so Windows can remove the program's folder. If a
  removal still fails, the folder is left over and removed at the next
  start, as for any folder in use.
- **No time limit of Pane's own** (#136, which replaced #18's provisional
  30 seconds; [current decisions](current-decisions.md) item 11): a helper
  runs until it exits, the command cancels it (its own timeout: the
  samples' "Echo within a second" races the run against a timer and drops
  it), the call that started it returns or the package stops. While a
  command waits on its helper, Pane serves every other extension's calls;
  only the command's own instance waits, since its calls run one after
  another. The samples' "Echo after a long wait" runs a helper for 40
  seconds. No user-facing cancel of a running action exists yet
  (generations: [no user cancellation](generations.md#what-stopping-cannot-do-yet)).
- Text only: input and output are UTF-8 strings, not binary data or
  streams, and a run answers once, when the helper exits. The helper's
  environment and working folder are fixed as above, and standard error is
  shown only for a failure.
- Targets: `x86_64` and `aarch64` on the three systems; 32-bit, musl versus
  glibc, minimum OS versions and library dependencies of a helper are not
  checked or claimed. A Linux helper linked against libraries the user's
  system lacks fails when started. The helper sample declares only the
  contributor baselines (`linux-x86_64`, `macos-aarch64`,
  `windows-x86_64`); on another machine, such as an Intel Mac or Linux on
  arm64, its helper is explained as unavailable and the helper tests stop
  saying so.
- A JavaScript or TypeScript promise cannot be cancelled: a run the
  command stops awaiting (a lost `Promise.race`) keeps its helper until the
  Pane call returns, which ends it.
- Stopping a call that awaits a helper still drops its instance, like any
  stopped call ([what stopping costs](generations.md#what-stopping-costs));
  cancelling a run from inside the guest does not.

## Author instructions

See [Native helpers](../guests/README.md#native-helpers) in the guests
README: writing the helper, building it for each target, declaring it in
`pane.json` and calling it from a command.

## Checks

- Runner ([`crates/pane-core/src/helpers/runner.rs`](../crates/pane-core/src/helpers/runner.rs)),
  with the real `pane-echo`: the answer; dropping a run ends its process
  without waiting; a generation ending ends it with nobody polling;
  stopping one instance's helpers leaves another's; stopping all, after
  which none starts; stopping those in a folder; a flood of output ends it;
  a program that cannot start; reading each system's program headers (ELF
  class and byte order, Mach-O thin and universal versus Java class files,
  PE, scripts refused) and explaining a file for another target, on every
  system; the `.exe` rule and absolute path for Windows, checked on every
  system; ids; symbolic links and files outside the package; the limits.
  Installing sets mode 0755 exactly and refuses a linked helper file, and
  an update retires the old copy before removing it
  ([`packages.rs`](../crates/pane-core/src/packages.rs)); dropping the last
  runtime handle ends a running helper
  ([`runtime.rs`](../crates/pane-core/src/runtime.rs)).
- Launcher public interface ([`crates/pane-core/tests/helpers.rs`](../crates/pane-core/tests/helpers.rs)),
  with the helper samples in Rust, JavaScript and TypeScript (one contract,
  run for each) and the real helper: its answer names this system;
  installing copies only this system's file, mode 0755; the preview lists
  the targets; a failing helper's exit code and standard error; an
  undeclared helper; a helper not built for this system, while the rest of
  the command works; a file for another system, a script or missing,
  refused at install; a file replaced after install (by one Pane does not
  recognize or a script), explained when run; cancelling a run; disabling,
  reloading, updating, uninstalling and quitting while the helper runs,
  each ending the process (Pane lists none, and the heartbeat file
  `pane-echo --wait` appends to every 20 ms stops growing: a process id is
  not trusted, since the system may reuse it once the helper is reaped),
  keeping the saved "started" note, and the new code running the helper
  again; three cycles of disable, reload and cancel; and invalid helper
  declarations (including `.cmd` and extensionless Windows files and bad
  ids).
- Native GUI smokes, two identical phases on all three systems. The first
  (screenshots 90 to 93, data folder `helper-data`): install the helper
  sample, run the helper (its answer names the system), cancel one after a
  second, start the waiting one, check with the system (`pgrep`,
  `Get-Process`) that it runs, disable the package and check that the
  process is gone, the note is kept, and no helper outlives Pane. The
  second (screenshot 94, data folder `helper-quit-data`): start the waiting
  helper, quit Pane the way the system asks an app to (on Linux a
  `WM_DELETE_WINDOW` message, sent by
  [`scripts/close_window.py`](../scripts/close_window.py); on Windows
  `CloseMainWindow`; on macOS a quit Apple event through
  `NSRunningApplication.terminate`), and check that Pane exits, no helper
  runs and its heartbeat stops. See the
  [Linux](platforms/linux.md#native-helpers-15), [macOS](platforms/macos.md#native-helpers-15)
  and [Windows](platforms/windows.md#native-helpers-15) notes for where it
  has run.
- These run in `cargo xtask ci`, which builds `pane-echo` natively for the
  system it runs on (`cargo xtask guests`) and runs clippy on it. CI runs
  it on each of its three systems (Windows Server 2025 x86-64, macOS 15
  arm64, Ubuntu 24.04 x86-64), so each system's tests run a real helper
  built for it; no helper is cross-compiled. When this was written they
  had run on Linux x86-64 only.
