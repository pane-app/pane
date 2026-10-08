# #172 evidence: applications' own icons on Windows

Captured and measured on 2026-10-08 from a release build of `main` at `ff9ad18d` (debug assertions on, default extensions from a local artifact source). Windows 11 build 26200, 96 DPI, opaque material, a scratch data and cache folder per run, and this computer's own installed applications.

## Icons

- `dark-root-applications.png` and `light-root-applications.png`: root search for "b". Each application row draws its own icon, bare and in its own colours.
- `dark-pinned-applications.png` and `light-pinned-applications.png`: applications pinned beside default commands.
- `dark-root-icons-blocked.png`: the same query with the icon cache's folder made unwritable (a deny ACL on the scratch folder). Rows keep their place with the placeholder.

## Typing with and without icons

`typing-with-icons.json` and `typing-without-icons.json` hold the raw results. Each is 162 real keystrokes (SendKeys): eight words typed and erased three times, in a fresh profile once the home folder's index and the icon refresh had settled. Each sample is the time from the key being sent to the first changed frame of the result list on screen, read through the desktop compositor. That includes composition (about one refresh), so the absolute figures are higher than the in-process benchmark's.

| | p50 | p90 | p95 | max |
| --- | --- | --- | --- | --- |
| Icons | 33.8 ms | 38.2 ms | 40.0 ms | 42.0 ms |
| Icons blocked | 34.2 ms | 38.4 ms | 38.7 ms | 43.3 ms |

The two are the same within the measurement's noise, so typing is as fast with icons as without.

## Quitting during icon extraction

The issue's open question was an exit hang while the real shell extraction ran. 16 runs closed the launcher's window, Pane's own quit path, at 0.1–20 s after start. In some runs this happened with real extraction in flight (2, 35 and 88 of 119 icons written). Every run exited in 49–423 ms. The hang did not reproduce.
