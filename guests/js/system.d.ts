// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `@pane/extension/system` (system.js): the clipboard,
// opening anything, revealing a path in the file manager, moving paths to
// the Recycle Bin, pasting into the application that was in front before
// Pane, that application and its selected text, through
// `pane:extension/system@0.1.0`
// (system-host.d.ts, wit/system.wit); and the standard actions built from
// them, which close the window after they act, as Raycast's built-in ones
// do.

import type { Action, Shortcut } from "./pane";

/** The system Pane runs on. */
export type HostSystem = "windows" | "macos" | "linux" | "other";

/** What `copy` puts on the clipboard: text, or a file by its absolute path. */
export type ClipContent = string | { text: string } | { file: string };

/** What the clipboard holds: text, or the first file copied in the file manager. */
export type Clip = { text: string; file?: never } | { file: string; text?: never };

/** A path {@link trash} did not move, and why. */
export interface NotTrashed {
  path: string;
  reason: string;
}

/** The system Pane runs on. */
export function runningOn(): HostSystem;

/** What the file manager is called: "Explorer", "Finder" or "File Manager". */
export function fileManagerName(): string;

/** What the trash is called: "Recycle Bin" on Windows, "Trash" elsewhere. */
export function trashName(): string;

/**
 * Puts `content` on the clipboard, replacing what was there. With
 * `concealed`, the copy carries the system's "do not record" marker, so
 * clipboard managers (Pane's own history among them) do not keep it.
 * Throws an `Error` saying why the system did not take it.
 */
export function copy(content: ClipContent, options?: { concealed?: boolean }): void;

/** What the clipboard holds now, or `null` when it holds neither text nor a file. */
export function readClipboard(): Clip | null;

/**
 * Opens `target`, unfiltered: a URL of any scheme (`https:`, `mailto:`,
 * `ms-settings:`, an application's own), a file, a folder or an
 * application, with the system's handler, or with `application` (a path,
 * or an installed application's id) when given. Throws an `Error` saying
 * why the system did not open it.
 */
export function open(target: string, application?: string | null): void;

/** Shows `path` selected in the file manager (File Explorer on Windows). */
export function showInFileManager(path: string): void;

/**
 * Thrown by {@link paste}, {@link frontApplication} and
 * {@link selectedText} where Pane cannot do them on this system yet (on
 * macOS and Linux, and on Windows until Pane's Windows power features
 * land). Not a failure: the command can do something else, as
 * {@link pasteAction} copies instead. Its message says what is not
 * available.
 */
export class NotAvailableError extends Error {}

/** The application that was in front before Pane. */
export interface FrontApplication {
  /** Its name, as the system shows it ("Notepad"). */
  name: string;
  /** Its icon: the system icon of its program, bundle or desktop entry. */
  icon: { file: string } | null;
}

/**
 * Closes the window, brings the application that was in front before Pane
 * back to the front and pastes `content` into it, then puts back what the
 * clipboard held unless something else was copied meanwhile. Throws a
 * {@link NotAvailableError} where Pane cannot paste yet (the window stays
 * open), or an `Error` saying why it failed.
 */
export function paste(content: ClipContent): void;

/**
 * The application that was in front before Pane, or `null` when there is
 * none. Throws a {@link NotAvailableError} where Pane cannot tell yet, or
 * an `Error` saying why it failed.
 */
export function frontApplication(): FrontApplication | null;

/**
 * The text selected in the application that was in front before Pane, or
 * `null` when nothing is selected there (not a failure). Throws a
 * {@link NotAvailableError} where Pane cannot read it yet, or an `Error`
 * saying why it failed.
 */
export function selectedText(): string | null;

/** Thrown by {@link trash}: the paths it did not move, each with why. */
export class TrashError extends Error {
  readonly notTrashed: NotTrashed[];
}

/**
 * Moves `paths` to the Recycle Bin (the trash elsewhere), from where the
 * user can put them back. Throws a {@link TrashError} naming those it did
 * not move.
 */
export function trash(paths: string | string[]): void;

/** How a standard action is titled, placed and finished. */
export interface StandardActionOptions {
  /** Its title instead of the standard one. */
  title?: string;
  /**
   * Keeps the window open after it acts: what it did is then said in a
   * toast instead of a HUD.
   */
  keepWindowOpen?: boolean;
  section?: string | null;
  shortcut?: Shortcut | null;
}

/**
 * Copy ("Copy to Clipboard"): copies `content` (concealed when asked),
 * closes the window and shows "Copied to Clipboard" in a HUD.
 */
export function copyAction(
  content: ClipContent,
  options?: StandardActionOptions & { concealed?: boolean },
): Action;

/** What {@link pasteAction} says in a HUD when it copied instead. */
export const PASTE_FALLBACK: string;

/**
 * Paste: closes the window and pastes `content` into the application that
 * was in front before Pane. Where Pane cannot paste yet, it copies
 * `content`, closes the window and says so in a HUD ("Copied — paste is not
 * available here yet"). It always closes the window: `keepWindowOpen` does
 * not apply.
 */
export function pasteAction(
  content: ClipContent,
  options?: Omit<StandardActionOptions, "keepWindowOpen">,
): Action;

/** Open: opens `target` (with `application` when given), then closes the window. */
export function openAction(
  target: string,
  options?: StandardActionOptions & { application?: string | null },
): Action;

/**
 * Open With…: a submenu of the installed applications, by name; the one
 * chosen opens `target`, then the window closes.
 */
export function openWithAction(target: string, options?: StandardActionOptions): Action;

/**
 * Show in Explorer ("Show in Finder" on macOS, "Show in File Manager"
 * elsewhere): shows `path` selected, then closes the window.
 */
export function showInFileManagerAction(path: string, options?: StandardActionOptions): Action;

/**
 * Move to Recycle Bin ("Move to Trash" elsewhere), in the destructive
 * style: moves `paths`, closes the window and says so in a HUD; fails
 * naming those it did not move.
 */
export function moveToTrashAction(
  paths: string | string[],
  options?: StandardActionOptions,
): Action;
