# Alerts: notifications, ringing sounds and the launcher

An extension's code often runs while Pane's window is hidden: a timer it
registered fires, a schedule runs, a service cycles. An alarm, a timer, a
Pomodoro or a reminder then has to get the user's attention. A HUD lasts
1.2 seconds, and a toast and a confirmation need the window. This page is
for extension authors who want to do that. The design is recorded in
[ADR 0053](adr/0053-extensions-get-the-users-attention-through-a-notification-a-ringing-sound-and-the-launcher.md)
(proposed).

An extension has three things for it:

- a **system notification**, posted through the system's own notification
  service, which a click turns into one of the extension's screens;
- a **ringing sound**, which keeps ringing until the extension stops it,
  and which never outlives the extension's code;
- the **launcher shown on one of its own commands**, such as a "Ringing"
  screen with Snooze and Dismiss: an **attention launch**.

The first two are the host interface `pane:extension/alerts`
([`wit/alerts.wit`](../wit/alerts.wit)). The third is no new call: it is
`launch` of `pane:extension/commands`
([`wit/commands.wit`](../wit/commands.wit)), user-initiated, as Raycast's
`launchCommand` with `LaunchType.UserInitiated` is from a background
command.

For the user, an alarm works like this:

- The alarm goes off while they work in another application. Pane's
  notification appears, under Pane's name with the extension's title as
  its attribution, and the alarm sound starts.
- Once they stop typing, Pane's launcher comes forward on the alarm's
  "Ringing" screen, with the keyboard focus. Enter snoozes or dismisses
  it. On Windows, clicking another application or the taskbar hides the
  launcher, as it hides whenever another application takes the focus
  ([hotkeys](hotkeys.md#what-a-press-does)). The sound keeps ringing,
  and the next opening within the reopening time shows "Ringing" again.
- Disabling or uninstalling the extension, or quitting Pane, stops the
  sound at once.

## The calls

### A system notification

`notify(notification)` posts a notification: a title, an optional
message and an optional click, all plain text. The click names one of the
calling package's own commands, by its id in `pane.json`, and the JSON
context its launch record carries.

- Clicking it launches that command user-initiated, with the context, as
  `launch` does. The click is the user's own action, so the launcher
  shows the command at once, as a hotkey does.
- Without a click, clicking it shows the launcher, as a second start of
  Pane does.
- It is silent: Pane asks the system to play no sound with it. A sound is
  `ring`'s job, so one alert never plays two sounds.
- A title over 200 characters or a message over 1,000 is cut with "…",
  never refused.
- It answers once the system took the notification, not once the user
  saw it.

### A ringing sound

`ring(sound, repeat, volume)` starts a sound and answers a `ringing`
handle the extension owns.

- **The sound**: `alarm` (insistent), `bell` or `chime` (gentle), which
  Pane makes itself and are the same on every system; `system`, the
  sound the system's own sound scheme plays for notifications; or a WAV,
  MP3 or Ogg Vorbis file, by its absolute path or one relative to the
  package's folder, at most 10 MiB.
- **How often**: `once`, a number of times from 1 to 100, or until
  stopped, back to back.
- **How loud**: 0 to 100, a percentage of the sound's full level at the
  output's own volume. Pane never changes the system's volume, and a
  muted output plays silently, which is a success.

The handle is an [owned registration](generations.md#owned-registrations)
(ADR 0041). There is no `stop`: dropping the handle stops the sound at
once. So does the instance holding it going away (a crash, a stopped
call, an unresponsive stop) and the package's generation ending (a
disable, a pause, a reload, an update, an uninstall, Pane quitting). A
disabled or crashed extension never rings forever, and the author writes
no cleanup.

The handle lives in the instance that rang. The command that stops it,
such as a "Ringing" screen's Dismiss, must therefore be served by the
same component, which keeps the handle in its state. The sample below
does that.

### Showing the launcher

A user-initiated `launch` asked from a call no window was shown for (a
timer's firing, a schedule, a service, the activation entry point) is an
**attention launch**:

- It may name only a view command of the caller's own package. Another
  package's command, a no-view command and a command that waits are
  refused, with the reason.
- `launch` answers once the launch is queued. Pane then shows the
  launcher on the command, with the keyboard focus, when the user can take
  it:
  - while the user is typing, in another application or in Pane, it waits
    for 1 second of no input, at most 10 seconds;
  - while an Alert (a confirmation) is pending on the launcher's screen,
    it waits until it is answered; any other screen makes way, and
    Settings stays open;
  - while Game mode holds Pane's hotkeys, it waits until the game leaves
    the front.
- Attention launches are shown one at a time, in the order asked: the
  next waits until the user leaves the screen the first opened. Each
  package has at most one waiting, and a newer one replaces it. A waiting
  one is dropped when its package is disabled, paused, reloaded, updated
  or uninstalled.
- The launch record is any command's launch: user-initiated, source
  `command`, and the context passed.

The notification and the sound are not delayed by any of this, so the
user is alerted while the launcher waits.

### Rust

[`pane_extension::alerts`](../guests/pane-extension/src/alerts.rs) has
`notify` and `ring`, with a `Notification` builder:

```rust
use pane_extension::alerts::{self, BundledSound, Notification, Repeat, Sound};

alerts::notify(
    &Notification::new("Tea is ready")
        .message("Steeped for 4 minutes")
        .on_click("ringing", Some(r#"{"timer":"tea"}"#)),
)?;
let ringing = alerts::ring(&Sound::Bundled(BundledSound::Alarm), Repeat::UntilStopped, 100)?;
// Keep `ringing` in the component's state; dropping it stops the sound.
```

A refusal is an `AlertsError`, whose `message()` says why.

### JavaScript and TypeScript

A command sets `"pane": { "alerts": true }` in its `package.json`, so the
build links the interface. It then imports
[`@pane-app/extension/alerts`](../guests/js/alerts.d.ts):

```ts
import { notify, ring } from "@pane-app/extension/alerts";

notify({
  title: "Tea is ready",
  message: "Steeped for 4 minutes",
  click: { command: "ringing", context: JSON.stringify({ timer: "tea" }) },
});
const ringing = ring("alarm", { repeat: "until-stopped", volume: 100 });
// Keep `ringing` in the module's state; ringing.dispose() stops the sound.
```

A refusal throws a `NotAvailableError`, a `LimitError` or an `Error`,
each with the reason. The host functions themselves are the module
`pane:extension/alerts@0.2.0`
([`guests/js/alerts-host.d.ts`](../guests/js/alerts-host.d.ts)), whose
refusals throw an object whose `payload` is the error.

Showing the launcher is `launch` of `pane:extension/commands@0.2.0`, in
every language:

```ts
import { launch } from "pane:extension/commands@0.2.0";

launch({ command: "ringing" }, "user-initiated", [], JSON.stringify({ timer: "tea" }));
```

## On each system

| | Windows | macOS | Linux |
|---|---|---|---|
| Notification | A toast (WinRT `ToastNotificationManager`), silent, under Pane's AppUserModelID; `not-available` when Windows does not know the id or notifications are turned off | The notification center (`UNUserNotificationCenter`), for `Pane.app` only: `not-available` for the bare `pane` program, until the user allows it, when turned off, and most likely for any unsigned Pane (CI's runner refused an unsigned bundle with `UNErrorDomain` 1) | The freedesktop notification service on the D-Bus session bus, on X11 and Wayland, silent (`suppress-sound`), the extension's title on the body's last line; `not-available` with no session bus or no service on it |
| Ringing sound | WASAPI in shared mode on a thread of Pane's own; `not-available` with no output device | An AudioToolbox audio queue on the default output device; `not-available` with none | ALSA's `default` device, which PipeWire, PulseAudio or the card serves; `not-available` without ALSA's library (`libasound.so.2`) or with no device |
| A `file` sound | WAV; MP3 through Media Foundation; Ogg Vorbis through Symphonia | WAV; MP3 and Ogg Vorbis through Symphonia | WAV; MP3 and Ogg Vorbis through Symphonia |
| The `system` sound | The sound scheme's notification sound | The user's alert sound from Sound settings (an AIFF file), else Tink | The freedesktop sound theme's notification sound (Ogg Vorbis on most distributions, or WAV); `not-available` when the theme is not installed |
| Attention launch | Waits for the user to stop typing (`GetLastInputInfo`) | Waits too (`CGEventSourceSecondsSinceLastEventType`) | Waits on X11 (its screen saver extension's idle time); shown without waiting on Wayland, which cannot tell |

On Windows:

- A toast shows only for a program Windows knows by its AppUserModelID.
  Pane uses `PaneApp.Pane` (`PaneApp.Pane.Development` for a debug
  build), sets it on its own process and registers it for the user at
  every start; the setup's Start menu shortcut carries it too.
- Under Focus assist or Do Not Disturb, Windows keeps the notification
  silent in the notification center. `notify` still answers success, and
  the extension cannot tell: Windows has no public way to read Focus
  assist's state. The ringing sound is not silenced by it.
- Notifications turned off for Pane, for the user or by policy answer
  `not-available`, saying which. Pane asks Windows' setting at each post,
  so turning notifications on while Pane runs counts from the next one.
- A device removed while a sound rings ends the sound, and the package's
  extension log says so.

On macOS, a notification needs Pane to run as `Pane.app`: macOS gives a
notification center only to an app, so the bare program (a development
build) answers `not-available` saying so. `Pane.app` asks the user once,
at the first post. Pane is not signed, and macOS most likely refuses an
unsigned app's notifications outright, as it did on CI's runner; the
answer then says so. The sound and the launcher still reach the user.

On Linux, the notification shows under Pane's name and icon (the
`pane.desktop` entry) and its click is the service's default action.
The desktop's own Do Not Disturb keeps it quiet, and `notify` still
answers success, as on Windows.

## Limits

- At most 10 notifications per package in a minute. The eleventh answers
  `limit`, and never reaches the system.
- At most 2 sounds ringing at once per package, and 8 in all. One more
  answers `limit`.
- Every ringing stops by itself after 10 minutes, however it was asked:
  an alarm nobody is there to hear does not ring all day. An extension
  that wants more rings again.
- A sound file is at most 10 MiB, WAV, MP3 or Ogg Vorbis.
- At most one attention launch waits per package.
- Nothing here works while Pane is not running. Pane starts at sign-in
  and lives in the tray, so an alarm fires whenever the user is signed
  in.

## Failures

Every refusal is an answer, never a crash, and none is a reason to pause
the extension:

- `not-available`: Pane cannot do it here, and nothing went wrong in the
  extension. There is no adapter on this system yet; notifications are
  turned off for Pane or the system; Windows does not know Pane's
  AppUserModelID; or there is no sound output device. It says which, for
  the user. The extension can do something else, such as still show the
  launcher.
- `limit`: one of the limits above refused it, and says which.
- `failed`: it went wrong. A sound file cannot be read or decoded,
  `times` is outside 1 to 100, a click names no command of the package
  or its context is not JSON, or the extension's code was replaced.
- `launch` refuses an attention launch it cannot make, with the reason,
  as it refuses any other launch.

The sample writes a refusal to its extension log and carries on, so the
user still gets whatever Pane could give.

Pane also writes every refused notification to its own log, `pane.log`
(the package's title, the answer and its reason, never the
notification's text): an installed package's extension log is kept in
memory only, so `pane.log` is where a user can see why a notification did
not come. On Windows, a toast Windows took but then could not show (its
`Failed` event, after `notify` answered success) is written there too.

## Author example and tests

### The samples

[`guests/sample-alerts`](../guests/sample-alerts) (Rust) is a short
alarm. Its two commands are served by one component:

- **Set Short Alarm** (no-view) registers a timer that fires once, 5
  seconds later, and says so in a HUD.
- When the timer fires, the sample starts the bundled alarm ringing until
  stopped, posts a notification whose click opens "Ringing", and launches
  "Ringing" user-initiated with the context `{"alarm":"Short alarm"}`.
- **Ringing** shows "Ringing: Short alarm" with **Snooze for 1 Minute**
  and **Dismiss** while the alarm rings. Both drop the ringing handle,
  which stops the sound; Snooze also sets the alarm again for a minute
  later. When nothing rings it says so.

[`guests/sample-alerts-js`](../guests/sample-alerts-js) and
[`guests/sample-alerts-ts`](../guests/sample-alerts-ts) are the same
commands in JavaScript and TypeScript. `cargo xtask js-guests` builds them
into `guests/prebuilt`, as it does the other JS/TS samples. Their
packages are `guests/packages/sample-alerts` and its `-js`/`-ts` copies.

### The checks

[`crates/pane-core/tests/alerts.rs`](../crates/pane-core/tests/alerts.rs)
runs the same checks in all three languages, with a clock the tests set,
a stand-in notifier that records what reaches it, and the runtime's
stand-in sound output, so nothing is shown on the desktop and nothing is
heard. It checks that:

- nothing rings or is posted before the alarm's 5 seconds are up;
- when they are, the alarm rings, owned by the package (its undo list
  holds the ringing), and the notification reaches the system with its
  title, message and the package's title as attribution, while the
  launcher stays hidden and opens nothing by itself;
- the attention launch then shows "Ringing" with the alarm's context,
  asking for the window and the keyboard focus;
- Dismiss stops the sound, lets go of the ringing and leaves nothing set;
- clicking the notification opens "Ringing" with the same context, at
  once;
- Snooze stops the sound, and a minute later, not before, the alarm rings,
  notifies and shows "Ringing" again;
- disabling the package stops the sound and drops the launch still
  waiting.

How each call behaves on its own is checked with test fixtures:
[`system_notifications.rs`](../crates/pane-core/tests/system_notifications.rs)
(the click, the cut text, the limit, the refusals),
[`ringing.rs`](../crates/pane-core/tests/ringing.rs) (every way the code
ends stops the sound, the limits, files, a lost device, the 10-minute
cap) and
[`attention_launch.rs`](../crates/pane-core/tests/attention_launch.rs)
(typing, a pending Alert, Game mode, the queue, the own-commands rule).
Windows' real toast and speakers are checked by hand.

## Tracking

Added for [#558](https://github.com/pane-app/pane/issues/558), part of
[#553](https://github.com/pane-app/pane/issues/553):
[#555](https://github.com/pane-app/pane/issues/555) (the Windows
notification), [#556](https://github.com/pane-app/pane/issues/556) (the
Windows ringing sound), [#557](https://github.com/pane-app/pane/issues/557)
(the attention launch) and [#559](https://github.com/pane-app/pane/issues/559)
(macOS and Linux).
