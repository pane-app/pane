# #189 evidence: the hidden phases measured on Windows, on the user's machine

Run on 2026-10-09 with `scripts/measure-windows.ps1`, with the user's
explicit consent (the machine is also used for games). Windows 11 build
26200 (26200.8737), x86_64, i5-14400F, 16 logical processors, 32 GB, NTFS
system drive, a standard user's process. Discord, a browser and a few
background apps ran through the runs; the load sat around 40–50%. No other
`pane` process ran, the launcher was hidden by the posted Escape, and the
launch-at-login registration was untouched. Each run: the development
build, a scratch profile (data, `LOCALAPPDATA`, file-index home all in a
new folder of the temporary folder, removed at the end), the five default
extensions served locally, settled 30 s, then 60 s sampled hidden, a sample
a second.

- `windows-after-26c8e224-summary.json` + `-record.json`: `main` at
  `26c8e224` (the state with #190 landed), the Windows baseline #189 asks
  for.
- `windows-before-d5534f45-summary.json` + `-record.json`: `d5534f45`, the
  parent of PR #274's merge (the state before #190 landed), built in a
  separate worktree — the 'before' half of #190's before/after pair.

| `hidden-idle` | before (`d5534f45`) | after (`26c8e224`) |
| --- | --- | --- |
| Whole-tree wake-ups a second | 495.19 | 388.39 |
| `pane-runtime-ep`, the epoch ticker | 99.68/s (6,042, 0.125 CPU s) | 0.0/s |
| `pane-runtime-wa`, the watchdog | 10.38/s (629, 0.016 CPU s) | 0.0/s |
| Whole-tree CPU share | 0.0062 | 0.0149 |
| Working set max | 103,344 KiB | 109,828 KiB |

The same numbers, with the Linux CI run beside them, are recorded in
[resource measurements](../../research/resource-measurements.md). The
summary files hold every thread's wake-ups and CPU; the record files hold
the machine details, the binary's SHA-256 and the parameters.
