// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// The system Pane runs on, for a JS/TS command (`@pane/extension/system`):
// the clipboard, opening anything, revealing a path in the file manager
// and moving paths to the Recycle Bin, through
// `pane:extension/system@0.1.0` (wit/system.wit); and the standard
// actions built from them, as Raycast's built-in ones behave (Copy, Open,
// Open With…, Show in Explorer, Move to Recycle Bin), which close the
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
  open as openTarget,
  readClipboard as read,
  reveal,
  runningOn as running,
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
