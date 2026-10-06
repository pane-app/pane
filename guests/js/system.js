// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// The system Pane runs on, for a JS/TS command (`@pane/extension/system`):
// the clipboard, opening anything, revealing a path in the file manager
// and moving paths to the Recycle Bin, pasting into the application that
// was in front before Pane, that application and the text selected in it,
// through `pane:extension/system@0.1.0` (wit/system.wit); and the standard
// actions built from them, as Raycast's built-in ones behave (Copy, Paste,
// Open, Open With…, Show in Explorer, Move to Recycle Bin), which close the
// window after they act. Bundled into the command that imports it, like
// any npm module.
//
// The host functions do only what they name. A standard action is a
// composition of them with `@pane/extension/feedback` (feedback.js), not
// part of Pane's contract (ADR 0037): an author can compose the steps
// differently.

import { installed } from "pane:extension/applications@0.1.0";
import {
  copy as put,
  frontApplication as front,
  open as openTarget,
  paste as pasteInto,
  readClipboard as read,
  reveal,
  runningOn as running,
  selectedText as selected,
  trash as recycle,
} from "pane:extension/system@0.1.0";
import { closeMainWindow, showHUD, showToast } from "./feedback.js";

/** Runs a host function, turning a refusal's `payload` text into an `Error`. */
function host(call) {
  try {
    return call();
  } catch (error) {
    const payload = /** @type {{ payload?: unknown }} */ (error)?.payload;
    throw typeof payload === "string" ? new Error(payload) : error;
  }
}

/** `content` as the WIT carries it: text, or `{ file }`. */
function wire(content) {
  if (typeof content === "string") return { tag: "text", val: content };
  if (content != null && typeof content.file === "string") return { tag: "file", val: content.file };
  if (content != null && typeof content.text === "string") return { tag: "text", val: content.text };
  throw new Error("copy needs text, { text }, or { file } with a file's absolute path");
}

/** The system Pane runs on: "windows", "macos", "linux" or "other". */
export function runningOn() {
  return running();
}

/** What the file manager is called: "Explorer", "Finder" or "File Manager". */
export function fileManagerName() {
  switch (running()) {
    case "windows":
      return "Explorer";
    case "macos":
      return "Finder";
    default:
      return "File Manager";
  }
}

/** What the trash is called: "Recycle Bin" on Windows, "Trash" elsewhere. */
export function trashName() {
  return running() === "windows" ? "Recycle Bin" : "Trash";
}

/**
 * Puts `content` (text, `{ text }` or `{ file }`) on the clipboard; with
 * `concealed`, marked so that clipboard managers do not keep it.
 */
export function copy(content, options = {}) {
  host(() => put(wire(content), Boolean(options?.concealed)));
}

/** What the clipboard holds: `{ text }`, `{ file }`, or `null`. */
export function readClipboard() {
  const clip = host(() => read());
  if (clip == null) return null;
  return clip.tag === "file" ? { file: clip.val } : { text: clip.val };
}

/**
 * Opens `target` (a URL of any scheme, a file, a folder or an
 * application), with `application` (a path or an installed application's
 * id) when given.
 */
export function open(target, application) {
  host(() => openTarget(String(target), application == null ? null : String(application)));
}

/** Shows `path` selected in the file manager. */
export function showInFileManager(path) {
  host(() => reveal(String(path)));
}

/**
 * Thrown by {@link paste}, {@link frontApplication} and
 * {@link selectedText} where Pane cannot do them on this system yet: not a
 * failure. Its message says what is not available.
 */
export class NotAvailableError extends Error {
  constructor(message) {
    super(message);
    this.name = "NotAvailableError";
  }
}

/**
 * Runs a host function answering `system-error`: its `not-available` is a
 * {@link NotAvailableError}, its `failed` an `Error` with the reason.
 */
function power(call) {
  try {
    return call();
  } catch (error) {
    const payload = /** @type {{ payload?: { tag?: string, val?: unknown } }} */ (error)?.payload;
    if (payload?.tag === "not-available") throw new NotAvailableError(String(payload.val));
    if (payload?.tag === "failed") throw new Error(String(payload.val));
    throw error;
  }
}

/**
 * Closes the window and pastes `content` (text, `{ text }` or `{ file }`)
 * into the application that was in front before Pane, then puts back what
 * the clipboard held. Throws a {@link NotAvailableError} where Pane cannot
 * paste yet (leaving the window open), or an `Error` saying why it failed.
 */
export function paste(content) {
  power(() => pasteInto(wire(content)));
}

/**
 * The application that was in front before Pane, `{ name, icon }`, or
 * `null` when there is none. `icon` is a file icon (`{ file }`) of its
 * program, bundle or desktop entry, ready for an item's icon, or `null`.
 * Throws a {@link NotAvailableError} where Pane cannot tell yet.
 */
export function frontApplication() {
  const app = power(() => front());
  if (app == null) return null;
  return { name: app.name, icon: app.icon == null ? null : { file: app.icon } };
}

/**
 * The text selected in the application that was in front before Pane, or
 * `null` when nothing is selected there (which is not a failure). Throws a
 * {@link NotAvailableError} where Pane cannot read it yet.
 */
export function selectedText() {
  const text = power(() => selected());
  return text == null ? null : text;
}

/** Thrown by {@link trash}: what was not moved, each with why. */
export class TrashError extends Error {
  constructor(notTrashed, of) {
    const items = of === 1 ? "item" : "items";
    const reasons = notTrashed.map((not) => `${not.path}: ${not.reason}`).join("; ");
    super(`Could not move ${notTrashed.length} of ${of} ${items} to the ${trashName()}: ${reasons}`);
    this.name = "TrashError";
    this.notTrashed = notTrashed;
  }
}

/** Moves `paths` (one or several) to the Recycle Bin; throws a {@link TrashError} naming those it did not move. */
export function trash(paths) {
  const all = (Array.isArray(paths) ? paths : [paths]).map(String);
  try {
    recycle(all);
  } catch (error) {
    const payload = /** @type {{ payload?: unknown }} */ (error)?.payload;
    if (Array.isArray(payload)) throw new TrashError(payload, all.length);
    throw typeof payload === "string" ? new Error(payload) : error;
  }
}

/**
 * What a standard action does once it has acted: closes the window, then,
 * with `hud`, says `said` in a HUD; or, kept open, says it in a toast.
 */
function finish(keepOpen, said, hud) {
  if (keepOpen) {
    showToast({ title: said });
    return;
  }
  closeMainWindow();
  if (hud) showHUD(said);
}

/** `options`' section and shortcut, for an action. */
function placed(options) {
  const more = {};
  if (options?.section != null) more.section = options.section;
  if (options?.shortcut != null) more.shortcut = options.shortcut;
  return more;
}

/**
 * Copy ("Copy to Clipboard"): copies `content`, closes the window and shows
 * "Copied to Clipboard" in a HUD.
 */
export function copyAction(content, options = {}) {
  return {
    title: options?.title ?? "Copy to Clipboard",
    onAction: async () => {
      copy(content, { concealed: options?.concealed });
      finish(options?.keepWindowOpen, "Copied to Clipboard", true);
    },
    ...placed(options),
  };
}

/** What Paste says in a HUD when it copied instead, where Pane cannot paste yet. */
export const PASTE_FALLBACK = "Copied — paste is not available here yet";

/**
 * Paste: closes the window and pastes `content` into the application that
 * was in front before Pane. Where Pane cannot paste yet, it copies
 * `content` instead, closes the window and says so in a HUD
 * ({@link PASTE_FALLBACK}). It always closes the window.
 */
export function pasteAction(content, options = {}) {
  return {
    title: options?.title ?? "Paste",
    onAction: async () => {
      try {
        paste(content);
      } catch (error) {
        if (!(error instanceof NotAvailableError)) throw error;
        copy(content);
        finish(false, PASTE_FALLBACK, true);
      }
    },
    ...placed(options),
  };
}

/** Open: opens `target`, with `options.application` when given, then closes the window. */
export function openAction(target, options = {}) {
  return {
    title: options?.title ?? "Open",
    onAction: async () => {
      open(target, options?.application);
      finish(options?.keepWindowOpen, "Opened", false);
    },
    ...placed(options),
  };
}

/** Open With…: a submenu of the installed applications; the one chosen opens `target`. */
export function openWithAction(target, options = {}) {
  return {
    title: options?.title ?? "Open With…",
    submenu: {
      title: "Open With",
      onOpen: async () => {
        const applications = host(() => installed());
        applications.sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
        return applications.map((application) => ({
          title: application.name,
          onAction: async () => {
            open(target, application.id);
            finish(options?.keepWindowOpen, `Opened with ${application.name}`, false);
          },
        }));
      },
    },
    ...placed(options),
  };
}

/** Show in Explorer (named for the system): shows `path` selected, then closes the window. */
export function showInFileManagerAction(path, options = {}) {
  return {
    title: options?.title ?? `Show in ${fileManagerName()}`,
    onAction: async () => {
      showInFileManager(path);
      finish(options?.keepWindowOpen, `Shown in ${fileManagerName()}`, false);
    },
    ...placed(options),
  };
}

/**
 * Move to Recycle Bin ("Move to Trash" elsewhere), destructive: moves
 * `paths`, closes the window and says so in a HUD; fails naming those it
 * did not move.
 */
export function moveToTrashAction(paths, options = {}) {
  return {
    title: options?.title ?? `Move to ${trashName()}`,
    style: "destructive",
    onAction: async () => {
      trash(paths);
      finish(options?.keepWindowOpen, `Moved to ${trashName()}`, true);
    },
    ...placed(options),
  };
}
