# Native validation — General: launch at login (#80)

**Status: plan only.** The native login integration cannot be exercised on
this branch's CI (every test runs a debug build, whose adapter — by
design — reports the development-install limitation and never touches the
runner's real startup list; the tests drive the toggle through a fake
adapter instead, exactly so no automated run ever changes a machine's
login configuration). Everything below is the capture plan the #84
native pass should follow on an available host, and this file must say
"passed/failed, evidence" per row once that run happens.

## What this ticket ships

The General page of the Settings window ([#80](https://github.com/hoangvu12/pane/issues/80)):
a launch-at-login switch over the platform's own login integration,
reached through the host-settings entity (`pane::settings`) and
`pane_core::autostart`'s adapters — the `Run` key under
`HKEY_CURRENT_USER` on Windows, `SMAppService`'s login item for the
application bundle on macOS, and the XDG autostart desktop entry on
Linux. The choice is recorded in `settings.json` beside the appearance
preferences; at start the saved choice is reconciled with the
registration the platform reports; the preference, the effective
registration and the integration's unavailability are kept distinct and
all three are shown; failed registration, removal or save is reported,
never shown as a successful toggle.

## What the fake-adapter tests already cover (CI)

`crates/pane/tests/settings.rs` drives the switch through the page's own
control with a scripted fake platform: the toggle's
choice-registration-record flow, repeat operations (one registration
replaced, never piled up), the restart that repairs a missing
registration and removes a stale one (a disabled choice never silently
enabled), a refused registration, a failed save that rolls the
registration back to what the record held, an unavailable integration
explained instead of offered, macOS's awaiting-approval state, and an
unreadable record that refuses the choice. `pane-core`'s unit tests
check the record's new field and the platform adapters' pure halves
(the Windows `Run` value's quoting, the Linux `Exec` line's quoting).

## What only native validation can observe

- **The registration itself, on each platform**: the `Run` value's
  presence and quoted command under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (readable with
  `reg query`), macOS's login item state through System Settings →
  General → Login Items, and `~/.config/autostart/pane.desktop`'s
  content. None of that is reachable from the test platform.
- **Whether the registered program actually starts at login** — the one
  behavior the whole ticket is about. Requires a real log-out/log-in (or
  a full restart) with the registration on, on each platform.
- **The development-install limitation as the real platform reports
  it**: a debug build's General page must show the limitation note and
  offer no working toggle; on macOS a release binary run outside its
  bundle must show the bundle note. This is the honesty the automated
  tests can only fake.
- **macOS's approval flow**: enabling on macOS 13+ leaves the
  registration awaiting the user's approval, which the page must say;
  approving in System Settings must clear the note without Pane
  re-registering (restart Pane after approving and check the note is
  gone).
- **Paths containing spaces**: install (or copy) the packaged program
  under a folder whose path contains spaces, enable, and read the
  registration's raw content — the Windows value must be one quoted
  command, the Linux `Exec` line one quoted command — then log in and
  confirm it starts.
- **The Linux desktop-environment caveat**: enable on an XDG-conforming
  desktop and confirm the entry starts at login; also record what a
  desktop that ignores autostart entries does (the page's note says
  Pane cannot see whether it was started — that stays true).

## Capture plan

Each row runs on a **release build from the platform's package**
(`cargo xtask package-windows` / `-macos` / `-linux`, then installed as
its README installs it), because the debug builds used for development
deliberately report the development-install limitation and manage no
registration. Use a scratch `PANE_DATA_DIR` per row so the settings
record is disposable. **Cleanup after each row**: turn the switch off
from the page and confirm the registration is gone (`reg query` /
Login Items list / `ls ~/.config/autostart`), then remove the scratch
data folder; any row that could not clean up through the page must be
finished by hand (`reg delete HKCU\...\Run /v Pane /f`,
System Settings → Login Items → remove Pane, `rm
~/.config/autostart/pane.desktop`) and say so in its evidence.

| Check | Evidence to capture |
| --- | --- |
| The General page is the page the window opens on, with the Startup group and the switch off by default | screenshot |
| Enabling registers the program: the Windows `Run` value exists and holds the quoted program path; the macOS login item appears (with the approval note until approved); the Linux entry exists with the quoted `Exec` | `reg query` output / Login Items screenshot / the entry file's content + screenshot of the note |
| The choice survives a restart of Pane (same data folder): the switch is on, the registration still there | screenshot + registration read after restart |
| A login with the registration on starts Pane (hidden launcher behavior is out of this ticket's scope; the process starting is the evidence) | screenshot of the running Pane after login, per platform |
| Disabling removes the registration and it stays off after a restart | registration read + screenshot |
| Repeated toggling leaves exactly one registration, never duplicates | registration read after several on/off cycles |
| The stale-registration reconciliation: enable, remove the registration by hand, restart Pane — it is repaired; disable by hand... leave the choice off with a registration planted by hand, restart — the registration is removed and the choice stays off | registration reads before/after each restart |
| A save that fails (make the record's replacement fail: a folder where `settings.json` belongs) rolls the registration back to what the record held and reports the failure | screenshot of the status + registration read |
| The development-install limitation: a debug build's page explains and offers no toggle; nothing is registered | screenshot + registration read showing no change |
| Paths with spaces: install under a spaced folder, enable, read the raw registration, log in | registration read + the after-login screenshot |
| The Linux caveat and the note's wording on a real desktop | screenshot |
| macOS approval: the note while pending, its clearing after the user approves, no re-registration at the next start | screenshots |

## Platform material limitations (documented, not invented)

- **Windows**: the registration is one value, `Pane`, in the user's own
  `Run` key — no administrator rights, no system-wide change. Whether
  the shell starts it is observable only by logging in. A value pointing
  at an older install is treated as this Pane's stale registration and
  rewritten when the choice is on.
- **macOS**: `SMAppService` (macOS 13+; the deployment target is older,
  so the class is looked up by name and a pre-13 macOS gets the
  limitation note). The registration is the application bundle itself;
  a non-bundled binary — any development build — cannot register, and
  the page says so. The user's approval in System Settings is the
  user's; Pane only reports that it is pending.
- **Linux**: the XDG autostart convention — `~/.config/autostart/pane.desktop`
  (or `$XDG_CONFIG_HOME/autostart/`). The major desktop environments
  honor it; not every desktop does, and nothing reports back. The page
  says the convention's limit instead of promising every desktop
  behaves alike.
- **All**: no run-as-service, no scheduled task, no system-level
  registration of any kind; nothing here needs elevated rights, and the
  toggle changes nothing but the user's own startup list.
