// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/files` in wit/files.wit: the files of
// the folder the user granted the command's package, which Pane lists for
// it, since a WASI guest has no folders to read. Only a command whose
// package.json sets `"pane": { "files": true }` imports it, and its
// package's pane.json sets `"folderAccess": true`, so that Pane offers its
// own "Choose folder…" row; the command never sees the folder's path.

/** `pane:extension/files@0.1.0`. */
declare module "pane:extension/files@0.1.0" {
  /** One file Pane found. */
  export interface FoundFile {
    /** Identifies the file to an `open-file` result: an opaque id, not a path. */
    id: string;
    /** Its path below the granted folder, with `/` between names, such as `notes/plan.md`. */
    relative: string;
  }

  /** What Pane found in the granted folder. */
  export interface FolderListing {
    /** The files, breadth first, each folder's entries in name order. */
    files: FoundFile[];
    /** Pane did not look at everything: it reached a limit, or a subfolder could not be read. */
    truncated: boolean;
  }

  /** How far Pane lists a granted folder. */
  export interface ScanLimits {
    depth: number;
    files: number;
    entries: number;
  }

  /**
   * `not-granted`: no folder is granted; `listing`: Pane is listing it and
   * will ask the command again once done; `ready`: the listing Pane keeps
   * for this visit of root search.
   */
  export type FolderState =
    | { tag: "not-granted" }
    | { tag: "listing" }
    | { tag: "ready"; val: FolderListing };

  /**
   * The granted folder's listing, at once, never waiting for it. On failure
   * (the folder is gone or can no longer be read) it throws an object whose
   * `payload` is the reason.
   */
  export function listFolder(): FolderState;

  /** Pane's scan limits, for telling the user. */
  export function limits(): ScanLimits;
}
