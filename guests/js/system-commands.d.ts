// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `@pane-app/extension/system-commands`
// (system-commands.js): the session and power commands — locking the
// screen, logging out, restarting, shutting down, sleeping, hibernating,
// turning the displays off and starting the screen saver — and the audio
// commands, the volume of the default output device and the microphones'
// mute, and the bin, appearance and device commands — the Recycle Bin
// (opening and emptying it), the system's appearance, HDR, the desktop,
// the file manager's hidden files, the removable drives and Bluetooth —
// through `pane:extension/system-commands@0.1.0`
// (system-commands-host.d.ts, wit/system-commands.wit). Each function
// answers what it ended in, never throwing for an operation that cannot
// happen on this system, or failed: show the text in a HUD
// (`@pane-app/extension/feedback`), and confirm the destructive ones
// first, as the System Commands default extension does (ADR 0040).

/**
 * What a command ended in: the state the system is in now (`done`, with
 * the text to show), or why nothing changed (`explained`, with why).
 */
export type Outcome = { state: "done"; text: string } | { state: "explained"; text: string };

/**
 * Locks the screen; the user's session stays as it is, and signing in
 * brings it back. Answers "Locking the screen".
 */
export function lockScreen(): Outcome;

/**
 * Logs the user out. Applications are asked to close and given the chance
 * to save, not forced (the user's choice, ADR 0040). Answers "Logging
 * out", or why nothing changed.
 */
export function logOut(): Outcome;

/**
 * Restarts the computer, forcing applications closed: an application with
 * unsaved work cannot hold a restart up (the user's choice, ADR 0040).
 * Answers "Restarting", or why nothing changed.
 */
export function restart(): Outcome;

/**
 * Powers the computer off, forcing applications closed as a restart does.
 * Answers "Shutting down", or why nothing changed.
 */
export function shutDown(): Outcome;

/**
 * Sleeps the computer: on one that enters Modern Standby when its
 * displays turn off, it turns them off and Windows enters its standby as
 * it does by itself; otherwise it suspends the computer. Answers
 * "Sleeping", or why nothing changed.
 */
export function sleep(): Outcome;

/**
 * Hibernates the computer, which a hibernation file makes possible;
 * without one, nothing changes and the answer says so ("Hibernation is
 * not available on this computer: there is no hibernation file").
 */
export function hibernate(): Outcome;

/**
 * Turns the displays off. Answers "Turning off the displays", or why
 * nothing changed.
 */
export function turnOffDisplays(): Outcome;

/**
 * Starts the screen saver the user chose; when none is set, nothing
 * changes and the answer says so. Answers "Starting the screen saver".
 */
export function startScreenSaver(): Outcome;

/**
 * Raises the volume of the default output device by the step Windows'
 * own volume keys take. Answers the volume it ended at, such as "Volume
 * 52%", or why nothing changed.
 */
export function volumeUp(): Outcome;

/**
 * Lowers the volume of the default output device by the same step.
 * Answers the volume it ended at, or why nothing changed.
 */
export function volumeDown(): Outcome;

/**
 * Mutes the default output device when it is not muted, unmutes it when
 * it is. Answers "Muted", or the volume it ended at as "Unmuted,
 * Volume 52%", or why nothing changed.
 */
export function toggleMute(): Outcome;

/**
 * Sets the volume of the default output device to `level`, 0 to 100; any
 * other level changes nothing and the answer says why. Answers the
 * volume it ended at.
 */
export function setVolume(level: number): Outcome;

/**
 * Mutes every microphone when any of them is unmuted, unmutes them all
 * otherwise. Answers "Microphones muted" or "Microphones unmuted", or
 * why nothing changed (no microphone is connected).
 */
export function toggleMicrophoneMute(): Outcome;

/**
 * Opens the Recycle Bin in the file manager. Answers "Opening the Recycle
 * Bin", or why nothing changed.
 */
export function openRecycleBin(): Outcome;

/**
 * Empties the Recycle Bin, what it holds deleted for good. An already
 * empty bin is a success the answer says. Answers "Emptied the Recycle
 * Bin" or "The Recycle Bin is already empty", or why nothing changed.
 */
export function emptyRecycleBin(): Outcome;

/**
 * Toggles the system's appearance between light and dark, the values for
 * applications and the system together. Answers "Light mode" or "Dark
 * mode", or why nothing changed.
 */
export function toggleAppearance(): Outcome;

/**
 * Toggles HDR: if any HDR-capable display is off they all turn on,
 * otherwise all off. Answers "HDR on" or "HDR off", or why nothing
 * changed (no HDR-capable display is connected).
 */
export function toggleHdr(): Outcome;

/**
 * Shows the desktop, as the shell's own toggle-desktop does: the windows
 * open now hide, or come back if it is showing. Answers "Showing the
 * desktop", or why nothing changed.
 */
export function showDesktop(): Outcome;

/**
 * Toggles whether the file manager shows hidden files, refreshing its
 * open windows. Answers "Hidden files shown" or "Hidden files hidden",
 * or why nothing changed.
 */
export function toggleHiddenFiles(): Outcome;

/**
 * Ejects every removable drive, each locked, dismounted and ejected; a
 * drive that fails is named with why, and none at all is explained.
 */
export function ejectRemovableDrives(): Outcome;

/**
 * Toggles Bluetooth: if any radio is off they all turn on, otherwise all
 * off. Answers "Bluetooth on" or "Bluetooth off", or why nothing changed
 * (no Bluetooth radio is connected).
 */
export function toggleBluetooth(): Outcome;
