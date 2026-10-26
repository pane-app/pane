// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/alerts` in wit/alerts.wit (ADR 0053):
// a system notification and a ringing sound the extension owns, made for
// a call no window was shown for, such as a timer's firing. Most commands
// use them through `@pane-app/extension/alerts` (alerts.d.ts). Only a
// command whose package.json sets `"pane": { "alerts": true }` imports
// it.

/** `pane:extension/alerts@0.2.0`. */
declare module "pane:extension/alerts@0.2.0" {
  /**
   * Why a call did not do what it asked, as the call throws it (an object
   * whose `payload` is one of these); none is a reason to pause the
   * extension:
   *
   * - `not-available`: Pane cannot do it here (no adapter on this system
   *   yet, notifications turned off for Pane or the system, no
   *   AppUserModelID Windows knows, or no sound output device);
   * - `limit`: one of Pane's limits refused it (2 ringing sounds per
   *   package and 8 in all, 10 notifications per package a minute);
   * - `failed`: it went wrong, such as a sound file that cannot be read.
   *
   * `val` says why for people.
   */
  export type AlertsError =
    | { tag: "not-available"; val: string }
    | { tag: "limit"; val: string }
    | { tag: "failed"; val: string };

  /**
   * What clicking a notification opens: one of the calling package's own
   * commands, launched user-initiated, with `context` (JSON text, at most
   * 1 MiB) in its launch record.
   */
  export interface NotificationClick {
    command: string;
    context?: string | null;
  }

  /** A system notification: plain text under Pane's name and icon. */
  export interface Notification {
    /** Cut with "…" past 200 characters. */
    title: string;
    /** Cut with "…" past 1,000 characters. */
    message?: string | null;
    /** Without one, a click shows the launcher. */
    click?: NotificationClick | null;
  }

  /**
   * Posts `notification` through the system's notification service,
   * silently. Returns once the system took it; otherwise it throws an
   * object whose `payload` is the {@link AlertsError}.
   */
  export function notify(notification: Notification): void;

  /** A sound Pane ships, the same on every system. */
  export type BundledSound = "alarm" | "bell" | "chime";

  /**
   * What `ring` plays: one of Pane's sounds, the system's notification
   * sound, or a WAV or MP3 file (absolute, or relative to the package's
   * folder; at most 10 MiB).
   */
  export type Sound =
    | { tag: "bundled"; val: BundledSound }
    | { tag: "system" }
    | { tag: "file"; val: string };

  /** How often `ring` plays its sound: `times` from 1 to 100. */
  export type Repeat =
    | { tag: "once" }
    | { tag: "times"; val: number }
    | { tag: "until-stopped" };

  /**
   * A ringing sound the command owns. `drop()` (or `using`) stops it at
   * once; so do the instance going away and the package's code being
   * disabled, paused, reloaded, updated or uninstalled. Every ringing
   * ends by itself after 10 minutes.
   */
  export class Ringing {
    private constructor();
    drop(): void;
    [Symbol.dispose](): void;
  }

  /**
   * Starts playing `sound` as `repeat` says, at `volume` (0 to 100),
   * answering the handle that stops it. Otherwise it throws an object
   * whose `payload` is the {@link AlertsError}.
   */
  export function ring(sound: Sound, repeat: Repeat, volume: number): Ringing;
}
