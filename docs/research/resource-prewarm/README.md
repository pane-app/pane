# Asking root search's providers ahead: the hidden phases on Windows

Run on 2026-10-24 on the user's machine, for the change that has Pane ask
the commands supplying indexed results (Applications) as soon as they can
run, so the first search after a fresh install or an update draws real
icons ([root search](../../root-search.md#when-they-are-asked)). The
user's go for this work included measuring it. Windows 11 build 26200
(26200.8737), x86_64, i5-14400F, 16 logical processors, 32 GB, the
development build. The user's own Pane was running (`otherPaneProcesses`:
1), as was another build on the machine, so the CPU shares are noisy; the
working sets are the numbers that matter here. The script's Start menu and
desktop are the user's own (about 115 applications), so the icons are
extracted for real, into the scratch cache.

Each run is `scripts/measure-windows.ps1` (the five defaults plus the three
Windows ones, served on 127.0.0.1; settled 30 s, sampled hidden for 60 s)
with two phases added by a throwaway copy of the script: once `hidden-idle`
ends, the scratch Pane is quit and started again on the same profile,
hidden the same way, settled 30 s (`restart-setup`) and sampled hidden for
60 s (`restart-idle`).

- `windows-before-*`: `main` at `13d097f7`.
- `windows-after-*`: `13d097f7` with this change (the record's `commit` is
  the checkout's HEAD, `13d097f7`, since the change was not committed yet;
  the binary's SHA-256 tells it apart).

| Working set max (KiB) | before | after |
| --- | --- | --- |
| `hidden-idle`: the first start, defaults just set up | 124,216 | 161,224 |
| `restart-idle`: the next start, same profile | 150,372 | 121,920 |

Two plain runs of the unchanged script before these gave 123,848 (before)
and 169,220 (after) for `hidden-idle`.

What moved:

- Before, nothing asked Applications in the first session (no
  `pane-applicatio` thread in `hidden-setup` or `hidden-idle`): its list and
  icons waited for the first search. The next start asked it at once (the
  window's root search home does), and the empty icon cache was filled
  then: 432 wake-ups a second for the icon threads in `restart-setup`, and
  about 26 MiB more in `restart-idle` than in `hidden-idle`.
- After, the first session lists the applications and extracts their icons
  right after setup (471 wake-ups a second in `hidden-setup`), and holds
  about 37 to 45 MiB more while idle. The next start finds every icon in
  the cache, extracts nothing and is the lowest of the four.

So the cost is the same one-time work (the applications' scan and the icon
extraction, whose shell and imaging libraries stay loaded), paid right
after setup rather than at the first search or the next start. A Pane
whose user has searched once holds it either way.
