// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/system-commands` in
// wit/system-commands.wit: the session and power commands — locking the
// screen, logging out, restarting, shutting down, sleeping, hibernating,
// turning the displays off and starting the screen saver — the audio
// commands, the volume of the default output device and the microphones'
// mute, and the bin, appearance and device commands: the Recycle Bin
// (opening and emptying it), the system's appearance, HDR, the desktop,
// the file manager's hidden files, the removable drives and Bluetooth.
// Most commands use them through
// `@pane-app/extension/system-commands`
// (system-commands.d.ts).

/** `pane:extension/system-commands@0.1.0`. */
declare module "pane:extension/system-commands@0.1.0" {
  /**
   * What a command ended in: the state the system is in now (`done`), or
   * why nothing changed (`explained`). Its text is what the command shows
   * the user.
   */
  export type Outcome = { tag: "done"; val: string } | { tag: "explained"; val: string };

  /** Locks the screen; the user's session stays as it is. */
  export function lockScreen(): Outcome;

  /**
   * Logs the user out. Applications are asked to close and given the
   * chance to save, not forced (the user's choice, ADR 0040).
   */
  export function logOut(): Outcome;

  /**
   * Restarts the computer, forcing applications closed (the user's
   * choice, ADR 0040).
   */
  export function restart(): Outcome;

  /** Powers the computer off, forcing applications closed as a restart does. */
  export function shutDown(): Outcome;

  /**
   * Sleeps the computer: on one that enters Modern Standby when its
   * displays turn off, it turns them off and Windows enters its standby;
   * otherwise it suspends the computer.
   */
  export function sleep(): Outcome;

  /**
   * Hibernates the computer, which a hibernation file makes possible;
   * without one, nothing changes and the answer says so.
   */
  export function hibernate(): Outcome;

  /** Turns the displays off. */
  export function turnOffDisplays(): Outcome;

  /**
   * Starts the screen saver the user chose; when none is set, nothing
   * changes and the answer says so.
   */
  export function startScreenSaver(): Outcome;

  /**
   * Raises the volume of the default output device by the step Windows'
   * own volume keys take, never past 100.
   */
  export function volumeUp(): Outcome;

  /** Lowers the volume of the default output device by the same step. */
  export function volumeDown(): Outcome;

  /** Mutes the default output device, or unmutes it. */
  export function toggleMute(): Outcome;

  /**
   * Sets the volume of the default output device to `level`, 0 to 100;
   * any other level changes nothing and the answer says why.
   */
  export function setVolume(level: number): Outcome;

  /**
   * Mutes every microphone when any of them is unmuted, unmutes them all
   * otherwise; a microphone that vanished since it was listed is
   * skipped.
   */
  export function toggleMicrophoneMute(): Outcome;

  /** Opens the Recycle Bin in the file manager. */
  export function openRecycleBin(): Outcome;

  /**
   * Empties the Recycle Bin, what it holds deleted for good; an already
   * empty bin is a success.
   */
  export function emptyRecycleBin(): Outcome;

  /**
   * Toggles the system's appearance between light and dark, the values
   * for applications and the system together, followed by the
   * setting-change broadcast with a hang timeout.
   */
  export function toggleAppearance(): Outcome;

  /**
   * Toggles HDR: the displays' advanced colour state; if any capable
   * display is off they all turn on, otherwise all off, and none capable
   * is explained.
   */
  export function toggleHdr(): Outcome;

  /** Shows the desktop, as the shell's own toggle-desktop does. */
  export function showDesktop(): Outcome;

  /**
   * Toggles whether the file manager shows hidden files, refreshing its
   * open windows.
   */
  export function toggleHiddenFiles(): Outcome;

  /**
   * Ejects every removable drive: each locked, dismounted and ejected,
   * with per-drive failures reported; none at all is explained.
   */
  export function ejectRemovableDrives(): Outcome;

  /**
   * Toggles Bluetooth: if any radio is off they all turn on, otherwise
   * all off; a radio that vanished since it was listed is skipped, and
   * none at all is explained.
   */
  export function toggleBluetooth(): Outcome;
}
