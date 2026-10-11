// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's system commands sample in TypeScript: a command that calls the
// session and power commands with `@pane-app/extension/system-commands`
// (#255) — Lock Screen, Log Out, Restart, Shut Down, Sleep, Hibernate,
// Turn Off Displays and Start Screen Saver — the audio commands (#265) —
// Volume Up, Volume Down, Toggle Mute, Set Volume and Toggle Microphone
// Mute — and the bin, appearance and device commands (#266) — Open
// Recycle Bin, Empty Recycle Bin, Toggle System Appearance, Toggle HDR,
// Show Desktop, Toggle Hidden Files, Eject Removable Drives and Toggle
// Bluetooth — one item for each, which runs it and says what it answered
// in a toast: the state the system ended in ("Sleep: Sleeping", "Volume
// Up: Volume 52%"), or why nothing changed ("Hibernate: Hibernation is
// not available on this computer: there is no hibernation file").
//
// The sample calls each host function as it is: no confirmation, no HUD —
// the System Commands default extension (pane-app/system-commands) is the
// one that composes them with ADR 0037's confirm and HUD host functions,
// and a test drives this sample with a fake system, so nothing it runs
// ever reaches the real one. Set Volume takes no argument of the sample's
// own: it sets the level the sample chose, standing in for the argument
// the System Commands extension declares. Items and answers match the
// Rust sample (guests/sample-system-commands) and the JavaScript one.
import { showToast } from "@pane-app/extension/feedback";
import {
  emptyRecycleBin,
  ejectRemovableDrives,
  hibernate,
  lockScreen,
  logOut,
  openRecycleBin,
  restart,
  setVolume,
  showDesktop,
  shutDown,
  sleep,
  startScreenSaver,
  toggleAppearance,
  toggleBluetooth,
  toggleHdr,
  toggleHiddenFiles,
  toggleMicrophoneMute,
  toggleMute,
  turnOffDisplays,
  volumeDown,
  volumeUp,
  type Outcome,
} from "@pane-app/extension/system-commands";

/** One item of the list: the host function it calls, by what it is called. */
interface Step {
  id: string;
  title: string;
  subtitle: string;
  ask: () => Outcome;
}

/** The items of the list, one per system command. */
const STEPS: Step[] = [
  {
    id: "lock-screen",
    title: "Lock Screen",
    subtitle: "Locks the screen; your session stays on",
    ask: lockScreen,
  },
  {
    id: "log-out",
    title: "Log Out",
    subtitle: "Ends your session; applications are not forced closed",
    ask: logOut,
  },
  {
    id: "restart",
    title: "Restart",
    subtitle: "Restarts the computer; applications are forced closed",
    ask: restart,
  },
  {
    id: "shut-down",
    title: "Shut Down",
    subtitle: "Powers the computer off; applications are forced closed",
    ask: shutDown,
  },
  {
    id: "sleep",
    title: "Sleep",
    subtitle: "Sleeps the computer, as its Modern Standby signal decides",
    ask: sleep,
  },
  {
    id: "hibernate",
    title: "Hibernate",
    subtitle: "Hibernates the computer, or says that it cannot",
    ask: hibernate,
  },
  {
    id: "turn-off-displays",
    title: "Turn Off Displays",
    subtitle: "Turns the displays off",
    ask: turnOffDisplays,
  },
  {
    id: "start-screen-saver",
    title: "Start Screen Saver",
    subtitle: "Starts the screen saver, or says that none is set",
    ask: startScreenSaver,
  },
  {
    id: "volume-up",
    title: "Volume Up",
    subtitle: "Raises the volume by the step Windows' volume keys take",
    ask: volumeUp,
  },
  {
    id: "volume-down",
    title: "Volume Down",
    subtitle: "Lowers the volume by the same step",
    ask: volumeDown,
  },
  {
    id: "toggle-mute",
    title: "Toggle Mute",
    subtitle: "Mutes the sound, or unmutes it, saying the volume it ended at",
    ask: toggleMute,
  },
  {
    id: "set-volume",
    title: "Set Volume",
    subtitle: "Sets the volume to 40, the level this sample chose",
    ask: () => setVolume(40),
  },
  {
    id: "toggle-microphone-mute",
    title: "Toggle Microphone Mute",
    subtitle: "Mutes every microphone when any is on, unmutes them all otherwise",
    ask: toggleMicrophoneMute,
  },
  {
    id: "open-recycle-bin",
    title: "Open Recycle Bin",
    subtitle: "Opens the Recycle Bin in File Explorer",
    ask: openRecycleBin,
  },
  {
    id: "empty-recycle-bin",
    title: "Empty Recycle Bin",
    subtitle: "Empties the Recycle Bin; an already empty one is a success",
    ask: emptyRecycleBin,
  },
  {
    id: "toggle-appearance",
    title: "Toggle System Appearance",
    subtitle: "Switches Windows between light and dark",
    ask: toggleAppearance,
  },
  {
    id: "toggle-hdr",
    title: "Toggle HDR",
    subtitle: "Turns HDR on when any capable display is off, otherwise off",
    ask: toggleHdr,
  },
  {
    id: "show-desktop",
    title: "Show Desktop",
    subtitle: "Hides the open windows, or brings them back",
    ask: showDesktop,
  },
  {
    id: "toggle-hidden-files",
    title: "Toggle Hidden Files",
    subtitle: "Shows hidden files in File Explorer, or hides them again",
    ask: toggleHiddenFiles,
  },
  {
    id: "eject-removable-drives",
    title: "Eject Removable Drives",
    subtitle: "Ejects every removable drive, reporting each drive that refused",
    ask: ejectRemovableDrives,
  },
  {
    id: "toggle-bluetooth",
    title: "Toggle Bluetooth",
    subtitle: "Turns Bluetooth on when any radio is off, otherwise off",
    ask: toggleBluetooth,
  },
];

/**
 * Says what `ask` answered in a toast: a success when it happened, a
 * failure when nothing changed. The toast is not returned: an action
 * resolves with nothing, since what it resolves with is what
 * `handle-event` answers, and that is text.
 */
const answer = (title: string, outcome: Outcome): void => {
  showToast({
    style: outcome.state === "done" ? "success" : "failure",
    title: `${title}: ${outcome.text}`,
  });
};

export const command = {
  async render() {
    return {
      title: "System commands",
      items: STEPS.map((step) => ({
        id: step.id,
        title: step.title,
        subtitle: step.subtitle,
        onAction: () => Promise.resolve(answer(step.title, step.ask())),
      })),
    };
  },
};
