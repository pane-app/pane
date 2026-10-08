// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/file-index` in wit/file-index.wit: the
// file index Pane keeps of the user's home folder (and the folders the user
// adds), current from the system's own change records, which a command
// searches instead of walking the disk (a WASI guest has no folders to
// read). Only a command whose package.json sets `"pane": { "fileIndex": true }`
// imports it, and its package's pane.json sets `"fileIndex": true`, so that
// Pane keeps the index current while the package is enabled. Each entry
// carries the id Pane gave it, its path, name, folder, kind, size and
// modified time; a root result or a search result names it by its id, and
// Pane checks it again before acting on it.

/** `pane:extension/file-index@0.1.0`. */
declare module "pane:extension/file-index@0.1.0" {
  /** What kind of entry it is; a link is listed, never followed. */
  export type EntryKind = "file" | "folder" | "link";

  /**
   * A kind of file, told from its name's extension: `text` is plain text
   * (notes, data, source code), `other` a file of none of the others.
   */
  export type Category =
    | "documents"
    | "images"
    | "audio"
    | "video"
    | "archives"
    | "applications"
    | "text"
    | "other";

  /**
   * How results are ordered: `relevance` (best match first; for a blank
   * query the most recently modified first) or `modified`.
   */
  export type Sort = "relevance" | "modified";

  /** What to search for besides the text. */
  export interface SearchOptions {
    /** Only entries of this kind. */
    kind?: EntryKind | null;
    /** Only files of this category. */
    category?: Category | null;
    sort: Sort;
    /** At most this many entries (Pane answers 200 at most per call). */
    limit: number;
    /** Skip this many first: the next page starts where the last ended. */
    offset: number;
  }

  /** An entry the index holds. */
  export interface FileEntry {
    /**
     * Names the entry to Pane, in an `open-file` root result or a search
     * result's `file`: an opaque id Pane gave it, not a path.
     */
    id: string;
    /** Its absolute path, for showing and copying. */
    path: string;
    /** Its own name. */
    name: string;
    /** Its folder for people: below the home folder as `~/…`. */
    folder: string;
    kind: EntryKind;
    /** Whether opening it would run a program, as its name says. */
    program: boolean;
    /** In bytes; 0 for a folder (a WIT `u64`, so a `bigint`). */
    size: bigint;
    /** Last modified, in whole seconds since 1970; 0 when unknown. */
    modified: bigint;
    /** The volume it is on, as the system numbers it. */
    volume: bigint;
  }

  /** Where the index is. */
  export type IndexState = "off" | "building" | "current" | "stopped";

  /** The index's state, for telling the user why results are incomplete. */
  export interface IndexStatus {
    state: IndexState;
    /** Entries in the index (an estimate while it changes). */
    entries: bigint;
    /** Entries the walk in progress has found so far. */
    found: bigint;
    /** Why it is off, waiting or stopped, or what to know about it. */
    reason?: string | null;
  }

  /**
   * The entries whose name (or folders) match `query`, best first; a blank
   * query lists the most recently modified. Answers at once from what is
   * indexed. On failure (the package does not declare `"fileIndex": true`)
   * it throws an object whose `payload` is the reason.
   */
  export function search(query: string, options: SearchOptions): FileEntry[];

  /** Where the index is now. */
  export function status(): IndexStatus;
}
