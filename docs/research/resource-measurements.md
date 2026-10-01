# Per-platform resource and latency measurements

Recorded 2026-09-30 for
[#4](https://github.com/hoangvu12/pane/issues/4) (the P3 prerequisite of
[the spec](https://github.com/hoangvu12/pane/issues/1)'s G7, US04, US14,
US81; T02, T24, T25). This is the record the platform pages link to for
resource and latency evidence and targets. Its status is honest and narrow:
the workload, the sampler, the targets file and the CI wiring exist, and
**no number has been measured yet** — the machine that wrote them has no
display, so the workload cannot run there, and CI's Linux leg has not run
it at the time of writing. Every target below is proposed in shape only,
pending the first collected record and the user's confirmation. No target
is inferred from the older CLI peaks ([the QuickJS spike's
measurements](qjs-p3-port-spike/README.md) stay what they are: same-workload
CLI comparisons, not launcher budgets); Q3's numerical budgets remain open
until the numbers exist and are accepted.

## The workload

One documented workload, run by
[`scripts/measure-linux.sh`](../../scripts/measure-linux.sh) under Xvfb on
the same runner, with the same guests and the same Pane binary as the smoke
([the Linux baseline](../platforms/linux.md)). It requires what
`scripts/smoke-linux.sh` requires (Xvfb, xdotool, Pillow, a Vulkan driver)
plus the guests built (`cargo xtask ci`). The phases are fixed; their
durations are the defaults (each has a `PANE_MEASURE_*` environment
override for a quick run, recorded in `record.json`):

| Phase | What happens | What it measures |
| --- | --- | --- |
| `cold-start` | Pane starts with a fresh data folder, nothing installed (screenshot 1, the smoke's hint-line check) | cold-start latency (exec → window appears), startup RSS/CPU |
| `warm-start` | three restarts with the same data folder | warm-start latency (median of three) |
| `idle-core` | Pane sits at root search, untouched, 60 s | idle whole-tree RSS and CPU |
| `installed-unused` | the seven packages calculator, quicklinks, applications, files, sample-rust, sample-js, sample-ts are installed, none invoked, 60 s | the cost of installed-but-unused extensions (T02); the applications extension's host-side scan of desktop entries runs, as it does for a user's Pane |
| `calculator` | the calculator, a default extension: an expression typed into root search, Enter copies the answer, Escape clears, ten times (the first answer is color-checked) | active-command use; repeated growth is the leak bound |
| `continuing-work` | the Watching continuing service ([services](../services.md), a cycle a second) and the Counting scheduled work ([schedules](../schedules.md), a run a minute) run while Pane sits at root search, 90 s | steady-state background work; their counts in the packages' content prove the work ran (asserted in the script), and screenshots 5 and 6 show both commands |
| `reload` | one package of its own in a fresh data folder: its component is swapped between the Rust and the JavaScript sample and reloaded six times, each reload waited for | resource release across repeated reload (T24) |
| `disable` | the same package disabled and enabled again five times, each state change waited for | resource release across repeated disable |

`record.json` records the machine and the build settings: the OS
(`/etc/os-release`), the kernel and architecture (`uname -srm`), the
commit, the Rust version, the Pane binary's path, size and profile, and the
workload's parameters. The default binary is the smoke's `target/debug/pane`:
a release build's first setup reaches for the not-yet-deployed artifact
source at start, so the release profile's workload waits for that source;
pass a release binary to the script when that day comes and the profile is
recorded either way.

## The record

The output folder holds, beside the workload's screenshots:

- `samples.jsonl` — one JSON line per sample (1 s cadence), each holding
  the phase, the root pid, and every process of Pane's whole tree (Pane
  and any helper it started) with its pid, parent, name, resident set size
  and CPU ticks, read from `/proc`
  ([`scripts/proc_tree.py`](../../scripts/proc_tree.py) `watch`);
- `events.jsonl` — the latencies and counts the workload itself measured
  (window latencies, how many service cycles and scheduled runs happened);
- `summary.json` — the per-phase aggregates (`summary` mode): RSS
  min/median/p95/max, CPU seconds and share, growth from the phase's first
  to last sample, the window latencies, and the derived per-extension
  cost, with `record.json` embedded as `environment`;
- `system.txt`, `stderr.log`, `xvfb.log` and `control` — the environment
  and what ran.

`proc_tree.py selfcheck` checks the sampling logic without a display
against a `sleep` process with children of its own (both tree-walk paths,
the summary's window/extra merging, and the check's pending/breach/unmeasured
behaviour); the workload runs it first. Linux only: `/proc` is where it
reads. The macOS and Windows legs need their own samplers (below).

## The targets

Proposed, **every number pending**; the machine-readable file is
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

Pending the next run of the branch: the numbers land in the
`resource-measurements` artifact (`measure/summary.json`) and are recorded
here once a run collects them. The runner is the `ubuntu-24.04` leg the
[Linux baseline](../platforms/linux.md) documents.

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

### macOS and Windows

Nothing measured, nothing wired: `/proc` is Linux-only, so their process-tree
samplers do not exist yet. When those legs measure, each should run the same
phases with the same metrics and its own targets file: a tree sampler
(macOS: the tree of pids and their RSS/CPU through `ps`/`proc_pidinfo`;
Windows: a PowerShell sampler of the process tree, or a Job Object if the
tree is to be contained), the window-appear latency for cold and warm
start, the same idle/installed/active/continuing/lifecycle phases, and the
same record shape so the platform pages stay comparable. This is recorded
as future work, not silently claimed; G7 needs each claimed combination's
own native results.

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
