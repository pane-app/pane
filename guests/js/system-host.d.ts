// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/system` in wit/system.wit: the
// clipboard, opening anything, revealing a path in the file manager,
// moving paths to the Recycle Bin, pasting into the application in front,
// that application and its selected text. Most commands use them through
// `@pane/extension/system` (system.d.ts), which also has the standard
// actions built from them.

/** `pane:extension/system@0.1.0`. */
declare module "pane:extension/system@0.1.0" {
  /** The system Pane runs on. */
  export type HostSystem = "windows" | "macos" | "linux" | "other";

  /** What is put on the clipboard, or read from it: text, or a file by its absolute path. */
  export type Clip = { tag: "text"; val: string } | { tag: "file"; val: string };

  /** A path `trash` did not move, and why. */
  export interface NotTrashed {
    path: string;
    reason: string;
  }

  /**
   * Why `paste`, `frontApplication` or `selectedText` did not answer:
   * `not-available` where Pane cannot do it on this system yet (not a
   * failure), `failed` when it went wrong; each with what to tell the user.
   */
  export type SystemError =
    | { tag: "not-available"; val: string }
    | { tag: "failed"; val: string };

  /** The application that was in front before Pane. */
  export interface FrontApp {
    /** Its name, as the system shows it. */
    name: string;
    /** The path (or `shell:` name) whose system icon is its icon. */
    icon?: string;
  }

  /** The system Pane runs on. */
  export function runningOn(): HostSystem;

  /**
   * Closes the window, pastes `content` into the application that was in
   * front before Pane and puts back what the clipboard held. Otherwise it
   * throws an object whose `payload` is a {@link SystemError}.
   */
  export function paste(content: Clip): void;

  /**
   * The application that was in front before Pane, or nothing. Otherwise
   * it throws an object whose `payload` is a {@link SystemError}.
   */
  export function frontApplication(): FrontApp | null | undefined;

  /**
   * The text selected in the application that was in front before Pane,
   * or nothing when nothing is selected. Otherwise it throws an object
   * whose `payload` is a {@link SystemError}.
   */
  export function selectedText(): string | null | undefined;

  /**
   * Puts `content` on the clipboard; with `concealed`, marked so that
   * clipboard managers (Pane's own history among them) do not keep it. On
   * failure it throws an object whose `payload` is the reason.
   */
  export function copy(content: Clip, concealed: boolean): void;

  /**
   * What the clipboard holds: its text, or the first file copied in the
   * file manager; nothing when it holds neither. On failure it throws an
   * object whose `payload` is the reason.
   */
  export function readClipboard(): Clip | null | undefined;

  /**
   * Opens `target` (a URL of any scheme, a file, a folder or an
   * application) with the system's handler, or with `application` (a path
   * or an installed application's id). On failure it throws an object
   * whose `payload` is the reason.
   */
  export function open(target: string, application: string | null | undefined): void;

  /**
   * Shows `path` selected in the file manager. On failure it throws an
   * object whose `payload` is the reason.
   */
  export function reveal(path: string): void;

  /**
   * Moves `paths` to the Recycle Bin (the trash elsewhere). If some were
   * not moved, it throws an object whose `payload` lists them, each with
   * why.
   */
  export function trash(paths: string[]): void;
}
