// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// The session and power commands for a JS/TS command
// (`@pane-app/extension/system-commands`), through
// `pane:extension/system-commands@0.1.0` (wit/system-commands.wit):
// locking the screen, logging out, restarting, shutting down, sleeping,
// hibernating, turning the displays off and starting the screen saver,
// the audio commands: the volume of the default output device and the
// microphones' mute, and the bin, appearance and device commands: the
// Recycle Bin (opening and emptying it), the system's appearance, HDR,
// the desktop, the file manager's hidden files, the removable drives and
// Bluetooth. Bundled into the command that imports it, like
// any npm module; only a command whose bundle uses it imports the
// interface. Each function answers what it ended in — the state the
// system is in now, or why nothing changed — never throwing for an
// operation that cannot happen on this system, or failed: show the text
// in a HUD (`@pane-app/extension/feedback`), and confirm the destructive
// ones first, as the System Commands default extension does (ADR 0040).

import {
  emptyRecycleBin as doEmptyRecycleBin,
  ejectRemovableDrives as doEjectRemovableDrives,
  hibernate as doHibernate,
  lockScreen as doLockScreen,
  logOut as doLogOut,
  openRecycleBin as doOpenRecycleBin,
  restart as doRestart,
  setVolume as doSetVolume,
  showDesktop as doShowDesktop,
  shutDown as doShutDown,
  sleep as doSleep,
  startScreenSaver as doStartScreenSaver,
  toggleAppearance as doToggleAppearance,
  toggleBluetooth as doToggleBluetooth,
  toggleHdr as doToggleHdr,
  toggleHiddenFiles as doToggleHiddenFiles,
  toggleMicrophoneMute as doToggleMicrophoneMute,
  toggleMute as doToggleMute,
  turnOffDisplays as doTurnOffDisplays,
  volumeDown as doVolumeDown,
  volumeUp as doVolumeUp,
} from "pane:extension/system-commands@0.1.0";

/**
 * What a command ended in: the state the system is in now (`done`, with
 * the text to show), or why nothing changed (`explained`, with why): an
 * object `{ state: "done" | "explained", text }`, as
 * `@pane-app/extension/system-commands`'s types say
 * (system-commands.d.ts).
 */

/** An answered host call as an outcome. */
function answer(outcome) {
  return outcome.tag === "done"
    ? { state: "done", text: String(outcome.val) }
    : { state: "explained", text: String(outcome.val) };
}

/**
 * Locks the screen; the user's session stays as it is, and signing in
 * brings it back. Answers "Locking the screen".
 */
export function lockScreen() {
  return answer(doLockScreen());
}

/**
 * Logs the user out. Applications are asked to close and given the chance
 * to save, not forced (the user's choice, ADR 0040). Answers "Logging
 * out", or why nothing changed.
 */
export function logOut() {
  return answer(doLogOut());
}

/**
 * Restarts the computer, forcing applications closed: an application with
 * unsaved work cannot hold a restart up (the user's choice, ADR 0040).
 * Answers "Restarting", or why nothing changed.
 */
export function restart() {
  return answer(doRestart());
}

/**
 * Powers the computer off, forcing applications closed as a restart does.
 * Answers "Shutting down", or why nothing changed.
 */
export function shutDown() {
  return answer(doShutDown());
}

/**
 * Sleeps the computer: on one that enters Modern Standby when its
 * displays turn off, it turns them off and Windows enters its standby as
 * it does by itself; otherwise it suspends the computer. Answers
 * "Sleeping", or why nothing changed.
 */
export function sleep() {
  return answer(doSleep());
}

/**
 * Hibernates the computer, which a hibernation file makes possible;
 * without one, nothing changes and the answer says so ("Hibernation is
 * not available on this computer: there is no hibernation file").
 */
export function hibernate() {
  return answer(doHibernate());
}

/**
 * Turns the displays off. Answers "Turning off the displays", or why
 * nothing changed.
 */
export function turnOffDisplays() {
  return answer(doTurnOffDisplays());
}

/**
 * Starts the screen saver the user chose; when none is set, nothing
 * changes and the answer says so. Answers "Starting the screen saver".
 */
export function startScreenSaver() {
  return answer(doStartScreenSaver());
}

/**
 * Raises the volume of the default output device by the step Windows'
 * own volume keys take. Answers the volume it ended at, such as "Volume
 * 52%", or why nothing changed.
 */
export function volumeUp() {
  return answer(doVolumeUp());
}

/**
 * Lowers the volume of the default output device by the same step.
 * Answers the volume it ended at, or why nothing changed.
 */
export function volumeDown() {
  return answer(doVolumeDown());
}

/**
 * Mutes the default output device when it is not muted, unmutes it when
 * it is. Answers "Muted", or the volume it ended at as "Unmuted,
 * Volume 52%", or why nothing changed.
 */
export function toggleMute() {
  return answer(doToggleMute());
}

/**
 * Sets the volume of the default output device to `level`, 0 to 100; any
 * other level changes nothing and the answer says why. Answers the
 * volume it ended at.
 * @param {number} level
 */
export function setVolume(level) {
  return answer(doSetVolume(level));
}

/**
 * Mutes every microphone when any of them is unmuted, unmutes them all
 * otherwise. Answers "Microphones muted" or "Microphones unmuted", or
 * why nothing changed (no microphone is connected).
 */
export function toggleMicrophoneMute() {
  return answer(doToggleMicrophoneMute());
}

/**
 * Opens the Recycle Bin in the file manager. Answers "Opening the Recycle
 * Bin", or why nothing changed.
 */
export function openRecycleBin() {
  return answer(doOpenRecycleBin());
}

/**
 * Empties the Recycle Bin, what it holds deleted for good. An already
 * empty bin is a success the answer says. Answers "Emptied the Recycle
 * Bin" or "The Recycle Bin is already empty", or why nothing changed.
 */
export function emptyRecycleBin() {
  return answer(doEmptyRecycleBin());
}

/**
 * Toggles the system's appearance between light and dark, the values for
 * applications and the system together. Answers "Light mode" or "Dark
 * mode", or why nothing changed.
 */
export function toggleAppearance() {
  return answer(doToggleAppearance());
}

/**
 * Toggles HDR: if any HDR-capable display is off they all turn on,
 * otherwise all off. Answers "HDR on" or "HDR off", or why nothing
 * changed (no HDR-capable display is connected).
 */
export function toggleHdr() {
  return answer(doToggleHdr());
}

/**
 * Shows the desktop, as the shell's own toggle-desktop does: the windows
 * open now hide, or come back if it is showing. Answers "Showing the
 * desktop", or why nothing changed.
 */
export function showDesktop() {
  return answer(doShowDesktop());
}

/**
 * Toggles whether the file manager shows hidden files, refreshing its
 * open windows. Answers "Hidden files shown" or "Hidden files hidden",
 * or why nothing changed.
 */
export function toggleHiddenFiles() {
  return answer(doToggleHiddenFiles());
}

/**
 * Ejects every removable drive, each locked, dismounted and ejected; a
 * drive that fails is named with why, and none at all is explained.
 */
export function ejectRemovableDrives() {
  return answer(doEjectRemovableDrives());
}

/**
 * Toggles Bluetooth: if any radio is off they all turn on, otherwise all
 * off. Answers "Bluetooth on" or "Bluetooth off", or why nothing changed
 * (no Bluetooth radio is connected).
 */
export function toggleBluetooth() {
  return answer(doToggleBluetooth());
}
