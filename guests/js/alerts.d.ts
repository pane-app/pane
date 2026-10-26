// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `@pane-app/extension/alerts` (alerts.js): a system
// notification and a ringing sound the command owns, through
// `pane:extension/alerts@0.2.0` (alerts-host.d.ts, wit/alerts.wit, ADR
// 0053). A command that imports it sets `"pane": { "alerts": true }` in
// its package.json. On a system Pane has no adapter for yet (macOS and
// Linux, for now), every call throws a {@link NotAvailableError}.

/**
 * Thrown where Pane cannot do it here: no adapter on this system yet,
 * notifications turned off for Pane or the system, no AppUserModelID
 * Windows knows for this Pane, or no sound output device. Not a failure:
 * the command can do something else. Its message says what.
 */
export class NotAvailableError extends Error {}

/**
 * Thrown when one of Pane's limits refused it: 10 notifications per
 * package a minute, 2 ringing sounds per package and 8 in all. Its
 * message says which.
 */
export class LimitError extends Error {}

/** A system notification. */
export interface Notification {
  /** Cut with "…" past 200 characters, never refused for its length. */
  title: string;
  /** More text under the title, cut with "…" past 1,000 characters. */
  message?: string | null;
  /**
   * What clicking it opens: one of the package's own commands, by its id
   * in `pane.json`, launched user-initiated with `context` (JSON text) in
   * its launch record. Without one, a click shows the launcher.
   */
  click?: { command: string; context?: string | null } | null;
}

/**
 * Posts `notification` through the system's notification service (a
 * toast on Windows), silently: a sound is {@link ring}'s job. Returns once
 * the system took it, not once the user saw it: under Focus assist or Do
 * Not Disturb it waits silently in the notification center, which is
 * still a success. Throws a {@link NotAvailableError} when it cannot be
 * posted at all, a {@link LimitError} past 10 a minute for the package,
 * and an `Error` when the click names no command of the package or its
 * context is not JSON.
 */
export function notify(notification: Notification): void;

/**
 * What {@link ring} plays: a sound Pane ships (`"alarm"`, insistent;
 * `"bell"`; `"chime"`, gentle), the system's own notification sound, or
 * a WAV or MP3 file by its absolute path or one relative to the package's
 * folder, at most 10 MiB.
 */
export type Sound = "alarm" | "bell" | "chime" | "system" | { file: string };

/** How {@link ring} plays its sound. */
export interface RingOptions {
  /**
   * How often, back to back: `"once"` (the default), a number of times
   * from 1 to 100, or `"until-stopped"`. Every ringing ends by itself
   * after 10 minutes, however it was asked.
   */
  repeat?: "once" | "until-stopped" | number;
  /**
   * 0 to 100, a percentage of the sound's full level at the output's own
   * volume (100 by default). Pane never changes the system's volume.
   */
  volume?: number;
}

/**
 * A ringing sound the command owns. `dispose()` (or `using`) stops it at
 * once; so do the instance holding it going away (a crash, a stopped
 * call) and the package's code being disabled, paused, reloaded, updated
 * or uninstalled. Keep it in the module's state, so the command that
 * stops it (a "Ringing" screen's Dismiss) is served by the same
 * component.
 */
export interface Ringing {
  /** Stops the sound at once. Idempotent; one that already ended stays ended. */
  dispose(): void;
  [Symbol.dispose](): void;
}

/**
 * Starts `sound` ringing as `options` says, on a thread of Pane's own.
 * Throws a {@link NotAvailableError} when there is no output device or no
 * adapter on this system yet, a {@link LimitError} when the package
 * already rings 2 sounds or Pane 8, and an `Error` when the file cannot be
 * read or decoded or `repeat` is out of its bounds.
 */
export function ring(sound: Sound, options?: RingOptions): Ringing;
