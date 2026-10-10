// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/typed-folder` in wit/typed-folder.wit:
// the entries of a folder the user typed into root search, which Pane's
// host lists for a command (a WASI guest has no folders to read), within
// bounds like the granted folder's scan policy: the direct entries only,
// folders first and each in name order, at most 500, a partial listing
// saying so. No folder is granted: the user named it. Only a command whose
// package.json sets `"pane": { "typedFolder": true }` imports it.

/** `pane:extension/typed-folder@0.1.0`. */
declare module "pane:extension/typed-folder@0.1.0" {
  /** One direct entry of the typed folder. */
  export interface FolderEntry {
    /**
     * Identifies the entry to an `open-file` result: an opaque id Pane
     * gave it for this listing, not a path.
     */
    id: string;
    /** The entry's own name, as Pane found it. */
    name: string;
    /** The entry is a folder. */
    folder: boolean;
    /**
     * Whether opening it would run a program (an executable, a script, a
     * shortcut, an installer), as its name or permissions say. File
     * search's own Enter shows such a file rather than run it.
     */
    program: boolean;
  }

  /** What Pane found in the typed folder. */
  export interface FolderListing {
    /** The direct entries, folders first and each in name order. */
    entries: FolderEntry[];
    /** Pane did not list everything: the folder holds more than 500 entries. */
    truncated: boolean;
  }

  /**
   * The entries of the folder `folder`, the path the user typed into root
   * search, resolved as a typed path is (`~` alone or followed by a
   * separator is the home folder, `file://` is taken off). On failure (not
   * a path, a network location, a file, or a folder Pane cannot read) it
   * throws an object whose `payload` is the reason.
   */
  export function listEntries(folder: string): FolderListing;
}
