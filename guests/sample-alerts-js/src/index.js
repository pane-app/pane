// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's alerts sample in JavaScript (ADR 0053): a short alarm that gets
// the user's attention while Pane's window is hidden. Commands, titles and
// HUDs match the Rust alerts sample (guests/sample-alerts) and the
// TypeScript one.
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
import { notify, ring } from "@pane-app/extension/alerts";
import { showHUD } from "@pane-app/extension/feedback";
import * as registrations from "@pane-app/extension/registrations";

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
let timer;
/**
 * The alarm's sound while it rings: this instance's own state.
 * @type {import("@pane-app/extension/alerts").Ringing | undefined}
 */
let sound;

/** Stops the sound, if it rings. */
function silence() {
  sound?.dispose();
  sound = undefined;
}

/** Sets the alarm to go off `seconds` from now. */
function setAlarm(seconds) {
  timer = registrations.after(seconds, goOff);
}

/** The alarm going off: from the timer's firing, a call no window was shown for. */
async function goOff() {
  // The sound, kept until Snooze or Dismiss disposes of it.
  silence();
  try {
    sound = ring("alarm", { repeat: "until-stopped", volume: 100 });
  } catch (error) {
    console.warn(`The alarm cannot ring: ${error.message}`);
  }
  try {
    notify({
      title: ALARM,
      message: "Snooze or dismiss it in Pane",
      click: { command: RINGING, context: CONTEXT },
    });
  } catch (error) {
    console.warn(`The alarm's notification was not posted: ${error.message}`);
  }
  // The launcher, shown on "Ringing" once the user can take it.
  try {
    launch({ command: RINGING }, "user-initiated", [], CONTEXT);
  } catch (error) {
    throw new Error(String(error?.payload ?? error));
  }
}

/** The alarm's name in `launch`'s context, if it names one. */
function alarmOf(launch) {
  try {
    const alarm = JSON.parse(launch?.context ?? "null")?.alarm;
    return typeof alarm === "string" ? alarm : undefined;
  } catch {
    return undefined;
  }
}

/** The events entry point: the timer's firing runs `goOff`. */
export const events = { handleEvent: registrations.handleEvent };

/** @type {import("@pane-app/extension").Command} */
export const command = {
  /** "Ringing": Snooze and Dismiss while the alarm rings. */
  async render(launch) {
    const alarm = alarmOf(launch);
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
  async run(id) {
    if (id !== "set") throw new Error(`unknown command: ${id}`);
    setAlarm(ALARM_SECONDS);
    showHUD(`Alarm set for ${ALARM_SECONDS} seconds`, "success");
  },
};
