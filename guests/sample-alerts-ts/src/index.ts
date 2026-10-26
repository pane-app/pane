// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's alerts sample in TypeScript (ADR 0053): a short alarm that gets
// the user's attention while Pane's window is hidden. Commands, titles and
// HUDs match the Rust alerts sample (guests/sample-alerts) and the
// JavaScript one.
//
// - "Set Short Alarm" (no-view) registers a timer that fires once, 5
//   seconds later, and says so in a HUD.
// - When it fires, from a call no window was shown for, the sample posts a
//   system notification whose click opens "Ringing", starts the bundled
//   alarm sound ringing until stopped, and shows the launcher on "Ringing"
//   (`launch`, user-initiated: an attention launch), passing the context
//   {"alarm":"Short alarm"}.
// - "Ringing" lists Snooze and Dismiss while the alarm rings. Both dispose
//   of the ringing, which stops the sound; Snooze also sets the alarm
//   again for a minute later. The ringing lives in this module's state, so
//   the command that stops it is served by the component that rang.
//
// package.json sets `"pane": { "events": true, "alerts": true }`: the
// events entry point delivers the timer's firing, and the alerts
// interface is imported.
import { launch } from "pane:extension/commands@0.2.0";
import type { Command, LaunchRecord, List } from "@pane-app/extension";
import { notify, ring, type Ringing } from "@pane-app/extension/alerts";
import { showHUD } from "@pane-app/extension/feedback";
import * as registrations from "@pane-app/extension/registrations";
import type { Handle } from "@pane-app/extension/registrations";

/** The "Ringing" command's id in `pane.json`. */
const RINGING = "ringing";
/** How long after "Set Short Alarm" the alarm goes off, in seconds. */
const ALARM_SECONDS = 5;
/** How long Snooze puts the alarm off, in seconds. */
const SNOOZE_SECONDS = 60;
/** The alarm's name, as the notification and "Ringing" say it. */
const ALARM = "Short alarm";
/** The context the notification's click and the launch pass "Ringing". */
const CONTEXT = JSON.stringify({ alarm: ALARM });

/** The alarm's timer while it is set, kept so it is not undone. */
let timer: Handle | undefined;
/** The alarm's sound while it rings: this instance's own state. */
let sound: Ringing | undefined;

/** Stops the sound, if it rings. */
function silence(): void {
  sound?.dispose();
  sound = undefined;
}

/** Sets the alarm to go off `seconds` from now. */
function setAlarm(seconds: number): void {
  timer = registrations.after(seconds, goOff);
}

/** Why `error` happened, for the extension log. */
function why(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** The alarm going off: from the timer's firing, a call no window was shown for. */
async function goOff(): Promise<void> {
  // The sound, kept until Snooze or Dismiss disposes of it.
  silence();
  try {
    sound = ring("alarm", { repeat: "until-stopped", volume: 100 });
  } catch (error) {
    console.warn(`The alarm cannot ring: ${why(error)}`);
  }
  try {
    notify({
      title: ALARM,
      message: "Snooze or dismiss it in Pane",
      click: { command: RINGING, context: CONTEXT },
    });
  } catch (error) {
    console.warn(`The alarm's notification was not posted: ${why(error)}`);
  }
  // The launcher, shown on "Ringing" once the user can take it.
  try {
    launch({ command: RINGING }, "user-initiated", [], CONTEXT);
  } catch (error) {
    throw new Error(String((error as { payload?: unknown })?.payload ?? error));
  }
}

/** The alarm's name in `record`'s context, if it names one. */
function alarmOf(record: LaunchRecord): string | undefined {
  try {
    const alarm: unknown = JSON.parse(record.context ?? "null")?.alarm;
    return typeof alarm === "string" ? alarm : undefined;
  } catch {
    return undefined;
  }
}

/** The events entry point: the timer's firing runs `goOff`. */
export const events = { handleEvent: registrations.handleEvent };

export const command: Command = {
  /** "Ringing": Snooze and Dismiss while the alarm rings. */
  async render(record: LaunchRecord): Promise<List> {
    const alarm = alarmOf(record);
    const title = alarm === undefined ? "Ringing" : `Ringing: ${alarm}`;
    if (sound === undefined) {
      return { title, items: [{ id: "quiet", title: "Nothing is ringing" }] };
    }
    return {
      title,
      items: [
        {
          id: "snooze",
          title: "Snooze for 1 Minute",
          onAction: async () => {
            silence();
            setAlarm(SNOOZE_SECONDS);
            showHUD("Snoozed for 1 minute", "success");
          },
        },
        {
          id: "dismiss",
          title: "Dismiss",
          onAction: async () => {
            silence();
            showHUD("Alarm dismissed", "success");
          },
        },
      ],
    };
  },

  /** "Set Short Alarm". */
  async run(id: string): Promise<void> {
    if (id !== "set") throw new Error(`unknown command: ${id}`);
    setAlarm(ALARM_SECONDS);
    showHUD(`Alarm set for ${ALARM_SECONDS} seconds`, "success");
  },
};
