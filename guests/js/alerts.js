// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Getting the user's attention outside Pane's window, for a JS/TS command
// (`@pane-app/extension/alerts`, ADR 0053): a system notification and a
// ringing sound the command owns, through `pane:extension/alerts@0.2.0`
// (wit/alerts.wit). A command that imports it sets
// `"pane": { "alerts": true }` in its package.json, so that it is built
// with the interface. Bundled into the command that imports it, like any
// npm module.
//
// Showing the launcher on one of the command's own screens from a call no
// window was shown for is `launch` of "pane:extension/commands@0.2.0",
// user-initiated (an attention launch), not part of this module.

import { notify as post, ring as start } from "pane:extension/alerts@0.2.0";

/**
 * Thrown where Pane cannot do it here: no adapter on this system yet,
 * notifications turned off, or no sound output. Not a failure: the
 * command can do something else. Its message says what is not available.
 */
export class NotAvailableError extends Error {
  constructor(message) {
    super(message);
    this.name = "NotAvailableError";
  }
}

/** Thrown when one of Pane's limits refused it. Its message says which. */
export class LimitError extends Error {
  constructor(message) {
    super(message);
    this.name = "LimitError";
  }
}

/** Runs a host function answering `alerts-error`, as an error of its kind. */
function host(call) {
  try {
    return call();
  } catch (error) {
    const payload = /** @type {{ payload?: { tag?: string, val?: unknown } }} */ (error)?.payload;
    if (payload?.tag === "not-available") throw new NotAvailableError(String(payload.val));
    if (payload?.tag === "limit") throw new LimitError(String(payload.val));
    if (payload?.tag === "failed") throw new Error(String(payload.val));
    throw error;
  }
}

/**
 * Posts a system notification: `{ title, message?, click? }`, where
 * `click` is `{ command, context? }`, one of the package's own commands
 * and the JSON text its launch record carries.
 */
export function notify(notification) {
  const click = notification?.click;
  host(() =>
    post({
      title: String(notification?.title ?? ""),
      message: notification?.message == null ? null : String(notification.message),
      click:
        click == null
          ? null
          : {
              command: String(click.command),
              context: click.context == null ? null : String(click.context),
            },
    }),
  );
}

/** `sound` as the WIT carries it. */
function sound(of) {
  if (of === "alarm" || of === "bell" || of === "chime") return { tag: "bundled", val: of };
  if (of === "system") return { tag: "system" };
  if (of != null && typeof of.file === "string") return { tag: "file", val: of.file };
  throw new Error('ring needs "alarm", "bell", "chime", "system" or { file } with a WAV, MP3 or Ogg Vorbis file');
}

/** `repeat` as the WIT carries it. */
function repeat(of) {
  if (of == null || of === "once") return { tag: "once" };
  if (of === "until-stopped") return { tag: "until-stopped" };
  if (typeof of === "number") return { tag: "times", val: of };
  throw new Error('repeat is "once", "until-stopped" or a number of times');
}

/**
 * Starts `of` ringing ("alarm", "bell", "chime", "system" or `{ file }`),
 * `options.repeat` times ("once" by default, "until-stopped", or 1 to
 * 100) at `options.volume` (0 to 100, 100 by default). Answers the
 * ringing: `dispose()` (or `using`) stops it.
 */
export function ring(of, options = {}) {
  const volume = options?.volume ?? 100;
  const handle = host(() => start(sound(of), repeat(options?.repeat), volume));
  let ringing = true;
  const stop = () => {
    if (!ringing) return;
    ringing = false;
    handle.drop();
  };
  return { dispose: stop, [Symbol.dispose]: stop };
}
