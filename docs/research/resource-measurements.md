# Per-platform resource and latency measurements

Recorded 2026-09-30 for
[#4](https://github.com/hoangvu12/pane/issues/4) (the P3 prerequisite of
[the spec](https://github.com/hoangvu12/pane/issues/1)'s G7, US04, US14,
US81; T02, T24, T25). This is the record the platform pages link to for
resource and latency evidence and targets. Its status is honest and narrow:
the workload, the sampler, the targets file and the CI wiring exist, and
the first record was collected on 2026-10-09 — the Linux workload in CI's
smoke job, the hidden phases on Windows on the user's machine (see
[Evidence](#evidence)); the machine that wrote them has no display, so it
measures nothing itself. Every target below is proposed in shape only,
pending the user's confirmation of the ceilings. No target
is inferred from the older CLI peaks ([the QuickJS spike's
measurements](qjs-p3-port-spike/README.md) stay what they are: same-workload
CLI comparisons, not launcher budgets); Q3's numerical budgets remain open
until the numbers exist and are accepted.

[#189](https://github.com/hoangvu12/pane/issues/189) (the measurement of
[#188](https://github.com/hoangvu12/pane/issues/188), idle cost) added the
`hidden-idle` phase, the wake-ups of every thread by its name, and a Windows
script for the hidden phase. Its baseline is recorded under
[Evidence](#evidence), with #190's before/after pair on Windows.

## The workload

One documented workload, run by
[`scripts/measure-linux.sh`](../../scripts/measure-linux.sh) under Xvfb on
the same runner, with the same guests and the same Pane binary as the smoke
([the Linux baseline](../platforms/linux.md)). It requires what
`scripts/smoke-linux.sh` requires (Xvfb, xdotool, Pillow, a Vulkan driver)
plus the guests built (`cargo xtask ci`), whose assembled packages the
hidden phases make the default extensions' five repositories from and
serve on 127.0.0.1 (as the smoke's first-setup phases serve them). Without
the guests the hidden phases are skipped, said
so on standard error and in `record.json` (`"skipped": ["hidden-idle"]`),
and `proc_tree.py check` reports `hidden-idle` as skipped rather than
failing; the other phases run and are checked as before. The phases are
fixed; their durations are the defaults (each has a
`PANE_MEASURE_*` environment override for a quick run, recorded in
`record.json`):

| Phase | What happens | What it measures |
| --- | --- | --- |
| `cold-start` | Pane starts with a fresh data folder, nothing installed (screenshot 1, the smoke's hint-line check) | cold-start latency (exec → window appears), startup RSS/CPU |
| `warm-start` | three restarts with the same data folder | warm-start latency (median of three) |
| `idle-core` | Pane sits at root search, untouched, 60 s | idle whole-tree RSS and CPU, and the wake-ups of each thread |
| `hidden-setup` | a data folder of its own: Pane starts (its one show), is hidden with Escape at its blank root search, acquires its five default extensions from their repositories, served on 127.0.0.1, and settles for 30 s (`PANE_MEASURE_SETTLE_SECONDS`) | nothing with a target: the setup the next phase follows |
| `hidden-idle` | Pane, hidden, with its default extensions and nothing else installed, untouched, 60 s (`PANE_MEASURE_HIDDEN_SECONDS`); the workload fails if the launcher shows itself | Pane's cost while hidden: whole-tree RSS and CPU share, and the wake-ups of each thread, the epoch ticker's and the watchdog's among them (#188) |
| `installed-unused` | the seven packages calculator, quicklinks, applications, files, sample-rust, sample-js, sample-ts are installed, none invoked, 60 s | the cost of installed-but-unused extensions (T02); the applications extension's host-side scan of desktop entries runs, as it does for a user's Pane |
| `calculator` | the calculator, a default extension: an expression typed into root search, Enter copies the answer, Escape clears, ten times (the first answer is color-checked) | active-command use; repeated growth is the leak bound |
| `continuing-work` | the Watching continuing service ([services](../services.md), a cycle a second) and the Counting scheduled work ([schedules](../schedules.md), a run a minute) run while Pane sits at root search, 90 s | steady-state background work; their counts in the packages' content prove the work ran (asserted in the script), and screenshots 5 and 6 show both commands |
| `reload` | one package of its own in a fresh data folder: its component is swapped between the Rust and the JavaScript sample and reloaded six times, each reload waited for | resource release across repeated reload (T24) |
| `disable` | the same package disabled and enabled again five times, each state change waited for | resource release across repeated disable |

In the hidden phases, Files' index covers an empty folder of the
workload's (`PANE_TEST_FILE_INDEX_HOME`, which development builds read), not
the runner's home, so `hidden-idle` measures Pane idling rather than a first
walk of a home folder; its watcher still runs. Whatever still runs after the
settling, such as the applications' icon extraction after a start, is part
of `hidden-idle`, and the per-thread counts say whose it is.

`record.json` records the machine and the build settings: the OS
(`/etc/os-release`), the kernel and architecture (`uname -srm`), the
commit, the Rust version, the Pane binary's path, size and profile, and the
workload's parameters. The default binary is the smoke's `target/debug/pane`:
a release build's first setup fetches its default extensions from the
repositories its committed pins name, so the release profile's workload
waits for a network the runner may not give;
pass a release binary to the script when that day comes and the profile is
recorded either way.

## The record

The output folder holds, beside the workload's screenshots:

- `samples.jsonl` — one JSON line per sample (1 s cadence), each holding
  the phase, the root pid, and every process of Pane's whole tree (Pane
  and any helper it started) with its pid, parent, name, resident set size
  and CPU ticks, and every thread of each process with its id, name,
  voluntary and involuntary context switches and CPU ticks, read from
  `/proc` ([`scripts/proc_tree.py`](../../scripts/proc_tree.py) `watch`);
- `events.jsonl` — the latencies and counts the workload itself measured
  (window latencies, how many service cycles and scheduled runs happened);
- `summary.json` — the per-phase aggregates (`summary` mode): RSS
  min/median/p95/max, CPU seconds and share, growth from the phase's first
  to last sample, the wake-ups (below), the window latencies, and the
  derived per-extension cost, with `record.json` embedded as
  `environment`;
- `system.txt`, `stderr.log`, `xvfb.log` and `control` — the environment
  and what ran.

`proc_tree.py selfcheck` checks the sampling logic without a display
against a `sleep` process with children of its own (both tree-walk paths,
the summary's window/extra merging, the threads counted by name, the
wake-up arithmetic on hand-made samples, a long name cut to 15 bytes, and
the check's pending/breach/unmeasured/skipped behaviour); the workload runs
it first. Sampling
is Linux only: `/proc` is where it reads. The Windows script writes samples
of the same shape itself (below); `summary` and `check` read either.

### Wake-ups by thread

Each phase's `wakeups` in `summary.json` counts how often the tree's threads
were woken over the phase:

```json
"wakeups": {
  "counted": "voluntary context switches",
  "names": "cut to 15 bytes, as Linux keeps them",
  "total": 6600,
  "per_second": 110.0,
  "by_thread": {
    "pane-runtime-ep": { "threads": 1, "count": 6000, "per_second": 100.0, "cpu_seconds": 0.04 },
    "pane-runtime-wa": { "threads": 1, "count": 600, "per_second": 10.0, "cpu_seconds": 0.01 }
  }
}
```

(The numbers show the shape only; nothing is measured yet.) On Linux the
count is a thread's voluntary context switches, `voluntary_ctxt_switches`
in `/proc/<pid>/task/<tid>/status`: each is the thread blocking (a sleep, a
lock, a read) and later being woken. A thread there at the phase's first
sample counts what it did since; a thread started during the phase counts
everything it did; threads of one name are added up, and `threads` says how
many there were. `cpu_seconds` is their CPU time over the phase, so a
thread that wakes rarely but computes is seen too.

The name is the thread's own: `std::thread::Builder::name` in Pane's code.
Linux keeps at most 15 bytes of it (the kernel's `comm`; Rust's standard
library cuts a longer name there), so a runtime thread's epoch ticker,
named `pane-runtime-epoch` (`crates/pane-core/src/runtime/deadlines.rs`,
`tick`), is listed as `pane-runtime-ep`, and its watchdog,
`pane-runtime-watchdog` (`runtime/supervisor.rs`, `watchdog`), as
`pane-runtime-wa`. Names that share their first 15 bytes are added up
together: `pane-extension-runtime` (the runtime thread itself) and
`pane-extension-check` both as `pane-extension-`, and
`pane-applications`, `pane-application-icons` and
`pane-application-changes` all as `pane-applicatio`. A thread nobody named
carries the process's name (`pane`). Windows keeps a thread's whole name;
`proc_tree.py summary` cuts every name to its first 15 bytes before it
counts, on both platforms (`"names"` in `wakeups` says so), so a Windows
and a Linux `summary.json` list the same thread under the same key.

## The Windows script

[`scripts/measure-windows.ps1`](../../scripts/measure-windows.ps1) measures
the hidden phases on Windows, for a scratch Pane on the machine it runs on.
It is not part of CI: it runs on the user's own machine, **only when the
user says the machine is free** (it is also used for games), and the
numbers it records then go under [Evidence](#evidence).

```powershell
cargo build -p pane
cargo xtask guests   # the default extensions' packages, made into the repositories the script serves
./scripts/measure-windows.ps1 -OutDir measure-windows
```

It starts the development build (`-Binary` names another) with
`PANE_DATA_DIR` and `LOCALAPPDATA` (where Pane keeps its cache) pointing
into a new folder of the temporary folder, for that one process only. The
scratch Pane acquires its five default extensions there from their
repositories, made from the guests' assembled packages and served on
127.0.0.1 by
[`scripts/repository_server.py`](../../scripts/repository_server.py) as
the smoke serves them, and Files' index covers an empty folder there
(without the guests the script skips the phase and reports so, in
`record.json`, as the Linux workload does). The
launcher's window shows once, as every start shows it, and is hidden at
once with Escape at its blank root search, posted to Pane's own window
(typed only if that did not hide it and Pane's window is the foreground
window). Then the script samples Pane's tree every second: `hidden-setup`
until the defaults are recorded and 30 s more (`-SettleSeconds`), then
`hidden-idle` for 60 s (`-HiddenSeconds`). Each sample reads the system's
process records once (`NtQuerySystemInformation`, `SystemProcessInformation`):
each process's CPU time and working set, and each thread's context switches
and CPU time, named by the thread's own name (`GetThreadDescription`; Rust's
standard library gives every named thread one). The samples have
`proc_tree.py`'s shape, and `proc_tree.py summary` turns them into the same
`summary.json`, with `record.json` (the Windows version and build, the
binary's path, size and SHA-256, the commit, the parameters) embedded.
There is no Windows targets file yet, so no `check` runs.

What it leaves alone: it never captures the screen and never brings Pane
forward. It never reads or writes the user's own Pane data or cache. A
scratch Pane reconciles the launch-at-login choice of its own settings with
the per-user startup list's `Pane` value at start, and would remove a value
it did not choose; so the script writes the scratch settings first with the
choice the list already holds, and checks the value afterwards, putting it
back if anything changed it. It quits only the Pane it started (closing its
window, then stopping it if it does not quit), and removes the scratch
folder, which holds what Clipboard History recorded during the run
(`-KeepScratch` keeps it). While it runs, the scratch Pane shows its tray
icon and takes the default hotkeys if no other Pane holds them. When the
user's own Pane (or any other `pane` process) runs, the script warns
before it starts and records how many there were
(`otherPaneProcesses`); it never touches them, but a second Pane's tray
icon, hotkeys and clipboard watch share the machine with the scratch one,
so a run with none is the one to record. The C# sampler is compiled
under a name made from a digest of its source, so the script can run
again in the same PowerShell session, after a change to it too.

On Windows a thread's count is every context switch to it
(`SYSTEM_THREAD_INFORMATION.ContextSwitches`), voluntary or not; for a
thread that mostly sleeps, that is its wake-ups. `rss_kb` is the working
set. `counted` in `wakeups` says which count a summary holds.

## The targets

Proposed, every ceiling pending (the measured rows await only the user's
confirmation, per [Evidence](#evidence)); the machine-readable file is
[`scripts/resource-targets.json`](../../scripts/resource-targets.json),
where a null ceiling means pending. The summary is checked against it at
the end of every workload run (`proc_tree.py check`): a confirmed ceiling
that the summary exceeds fails the run; a pending one is reported and
passes.

| Metric | `summary.json` field | Target | Status |
| --- | --- | --- | --- |
| Cold-start latency (exec → window) | `cold-start.window_ms.median` | ceiling | pending a measurement |
| Warm-start latency (median of three) | `warm-start.window_ms.median` | ceiling | pending a measurement |
| Idle whole-tree RSS | `idle-core.rss_kb.max` | ceiling | pending a measurement |
| Idle whole-tree CPU share | `idle-core.cpu_share` | ceiling | pending a measurement |
| Idle wake-ups a second (every thread of the tree) | `idle-core.wakeups.per_second` | ceiling | measured (Linux CI), ceiling pending the user's confirmation |
| Hidden whole-tree RSS (default extensions) | `hidden-idle.rss_kb.max` | ceiling | measured (Linux CI, Windows user's machine), ceiling pending the user's confirmation |
| Hidden whole-tree CPU share | `hidden-idle.cpu_share` | ceiling | measured (Linux CI, Windows user's machine), ceiling pending the user's confirmation |
| Hidden wake-ups a second (every thread of the tree) | `hidden-idle.wakeups.per_second` | ceiling | measured (Linux CI, Windows user's machine), ceiling pending the user's confirmation |
| Additive cost per installed-but-unused extension | `installed-unused.per_extension_kb` | ceiling | pending a measurement |
| Installed-but-unused RSS | `installed-unused.rss_kb.max` | ceiling | pending a measurement |
| Active-command RSS (calculator, repeated) | `calculator.rss_kb.max` | ceiling | pending a measurement |
| Active-command growth (leak bound) | `calculator.growth_kb` | ceiling | pending a measurement |
| Continuing-work RSS (service + schedule) | `continuing-work.rss_kb.max` | ceiling | pending a measurement |
| Continuing-work growth | `continuing-work.growth_kb` | ceiling | pending a measurement |
| Resource release across repeated reload | `reload.growth_kb` | ceiling | pending a measurement |
| Resource release across repeated disable | `disable.growth_kb` | ceiling | pending a measurement |

How a number becomes a target: the workload runs on CI's Linux leg
(wired in [ci.yml](../../.github/workflows/ci.yml), the record uploaded as
the `resource-measurements` artifact); the measured values are recorded
here as this machine's data point; a ceiling is proposed from them with
headroom and a stated rationale; **the user confirms it** (Q3's budgets
stay open until then); the number lands in `scripts/resource-targets.json`
and the check enforces it from then on. A breach is narrow corrective work
on the named metric, never an open-ended optimization ticket. Workload
integrity (that the service actually cycled, that the schedule actually
ran, that each reload actually replaced the managed copy) is asserted by
the workload itself, not by the targets.

## Evidence

### Linux, CI (x86_64, Xvfb)

Recorded from the release matrix's smoke job after the workload it appends
([run 37907199550](https://github.com/pane-app/pane/actions/runs/37907199550),
`main` at `26c8e224`, 2026-10-09, kernel 6.17.0-1022-azure; the
`resource-measurements` artifact holds the whole record). The phases it
measured, per phase: whole-tree CPU share, working-set maximum and
wake-ups a second. Ceilings stay pending the user's confirmation.

| Phase | CPU share | RSS max (KiB) | Wake-ups/s |
| --- | --- | --- | --- |
| cold-start | 0.2195 | 248,160 | 268.37 |
| warm-start | 0.1297 | 248,892 | 210.58 |
| idle-core | 0.0014 | 276,820 | 64.51 |
| hidden-setup | 0.0140 | 287,604 | 18.02 |
| hidden-idle | 0.0002 | 296,972 | 3.99 |
| installed-unused | 0.0135 | 260,952 | 74.80 |
| calculator | 0.1487 | 295,596 | 169.35 |
| continuing-work | 0.0298 | 355,264 | 98.10 |
| reload | 0.1344 | 361,444 | 398.72 |
| disable | 0.1830 | 355,960 | 519.42 |
| install-unused-packages | 0.1685 | 289,372 | 340.38 |
| install-continuing-work | 0.1497 | 306,644 | 314.63 |
| install-lifecycle-package | 0.0499 | 266,532 | 103.79 |

The `hidden-idle` baseline (#189): 3.99 wake-ups a second across the whole
tree (236 over the phase), and only two threads woke at all — `blocking-1`
and `pane-file-watch` at 2.0 a second each. `pane-runtime-ep` (the epoch
ticker) and `pane-runtime-wa` (the watchdog) sat at **0.0 wake-ups a
second**, in `hidden-idle` and in `idle-core` alike: the condvar sleep of
#190 holding while nothing runs. The runtime's other named threads, the
extension threads, the file index and every swapchain thread also sat at
zero; `idle-core`'s 64.51 a second is mostly the renderer's unnamed
threads.

### Linux, this machine (aarch64, no display)

Ubuntu 24.04.5 LTS, kernel 5.15.0-1081-oracle, aarch64, 3 CPUs, no
display and no Xvfb: the GUI workload cannot run here, so no aarch64
numbers exist. What ran here: `proc_tree.py selfcheck` (a `sleep` process
with children, both tree-walk paths, the summary and check behaviour), the
whole record path with a synthetic record (`summary` and `check`, including
the per-extension derivation and a deliberate breach failing), a full
stubbed dry run of the workload's mechanics end to end (every phase, the
sampler wiring, the record and the check; no GUI, so no measurement), and
`bash -n` on both scripts.
aarch64 GUI numbers would need the smoke's environment on an aarch64
machine; only the x86_64 CI leg is claimed.

### Windows, the user's machine

[The Windows script](#the-windows-script) ran for the first time on
2026-10-09, with the user's explicit consent (the machine is also used for
games), twice: the baseline of `main` at `26c8e224`, and the state before
PR #274 landed (`d5534f45`, the parent of its merge) built in a separate
worktree, for #190's before/after pair. Windows 11 build 26200 (26200.8737),
i5-14400F, 16 logical processors, 32 GB, the development build, hidden by
the posted Escape; no other `pane` process ran, and the launch-at-login
registration was untouched. Discord, a browser and a few background apps
ran (load about 40–50%). Each run: 60 s sampled hidden after 30 s of
settling, samples a second; the whole records are in
[`docs/evidence/resource-189`](../evidence/resource-189).

| `hidden-idle` | before `d5534f45` | after `26c8e224` (main) |
| --- | --- | --- |
| Whole-tree wake-ups a second | 495.19 | 388.39 |
| `pane-runtime-ep` (the epoch ticker) | **99.68** (6,042, 0.125 CPU s) | **0.0** |
| `pane-runtime-wa` (the watchdog) | **10.38** (629, 0.016 CPU s) | **0.0** |
| Whole-tree CPU share | 0.0062 | 0.0149 |
| Working set max (KiB) | 103,344 | 109,828 |

The before numbers are exactly the threads' periods: the 10 ms tick at
99.68 a second and the 100 ms look at 10.38 a second; after PR #274 both
sleep while no guest call runs, and the tree's whole wake-up rate drops by
about the same 110 a second. What still wakes on Windows, before and after
alike, is `VSyncProvider` at about 306 a second — the renderer's own
thread, not one of #188's — the 58 unnamed `pane` threads at about 69 a
second together, and the file watcher at about 10. The Windows baseline of
`main` (#189) is therefore the after column above. The other phases have
no Windows sampler or workload yet.

### macOS, and the rest of Windows

Nothing measured, nothing wired for macOS: `/proc` is Linux-only, so its
process-tree sampler does not exist yet, and Windows has only the hidden
phases. When those legs measure, each should run the same phases with the
same metrics and its own targets file: a tree sampler (macOS: the tree of
pids and their RSS/CPU through `ps`/`proc_pidinfo`; Windows: the script's
sampler, or a Job Object if the tree is to be contained), the
window-appear latency for cold and warm start, the same
idle/installed/active/continuing/lifecycle phases, and the same record
shape so the platform pages stay comparable. This is recorded as future
work, not silently claimed; G7 needs each claimed combination's own native
results.

## Release validation

[The release checklist](https://github.com/hoangvu12/pane/issues/57)'s row
"Run the documented P3 resource workload on this platform, measuring the
full process tree and comparing with its recorded targets" runs, on Linux:

```sh
bash scripts/measure-linux.sh measure
python3 scripts/proc_tree.py check measure/summary.json scripts/resource-targets.json
```

CI's Linux leg runs both on every push; the release validation rechecks
the candidate's own run and record. The Windows and macOS rows stay open
until those legs measure (above). Recorded here because this ticket does
not edit the issue.

## Known limits

- The measured combination is Xvfb with software Vulkan (lavapipe), the
  [Linux baseline](../platforms/linux.md)'s — not a desktop session, not a
  real GPU, and the numbers say so in `record.json`.
- The default binary is the development profile (the smoke's); the release
  profile's numbers wait for the deployed artifact source (above).
- A 1 s cadence over 60–90 s phases catches steady state and gross growth,
  not sub-second spikes. Window latencies are wall-clock to the window's
  appearance, polled at 0.2 s, so they quantize by about that much and are
  an X11-under-Xvfb property, not a desktop-session perception number.
- One runner is one data point; variance across runners is unknown until
  several runs are collected, and CPU share can be inflated by other work
  on the runner.
- Warm start measures a restart of a Pane with nothing installed; a warm
  start with nine packages installed is part of the `installed-unused` and
  `continuing-work` phases' start, recorded in their samples.
- `hidden-idle` follows a first setup: the defaults were acquired in the
  same run, so what a start does once (the applications' icons) may still
  run after the settling, and is counted there by thread. No update of
  Pane itself is offered there: neither workload's hidden phase names an
  application update source, so nothing offers a newer Pane (CI's smoke
  leaves one in the artifacts it serves, which the hidden phases never
  read).
- Wake-ups are counted at the sample cadence: a thread that ends between
  two samples loses the switches since its last one. Names that share their
  first 15 bytes are one entry, on both platforms (above).
- On Windows the scratch `LOCALAPPDATA` moves more than Pane's cache and
  log: the ClickOnce store (`%LOCALAPPDATA%\Apps\2.0`), where Pane finds a
  ClickOnce application's icon (`applications/icons/click_once.rs`), and
  the folder Windows makes for each packaged application
  (`%LOCALAPPDATA%\Packages`), which Pane watches for a package's folder
  appearing or going (`applications/start_menu.rs`), are the scratch
  folder's, which holds neither. So the Windows `hidden-idle` leaves out
  what those two cost on a user's own Pane: the ClickOnce icon lookups,
  and the wake-ups of the watch on the user's `Packages` folder, which on
  a real profile holds a folder per installed package. The Start menu
  folders and the desktop are the user's own, as for any Pane.
