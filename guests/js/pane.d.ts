// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Types for Pane's extension contract, `pane:extension/command` in
// wit/extension.wit, as JavaScript and TypeScript commands see it. They
// describe plain values only; nothing here is specific to the JS engine.
// A command's list reaches Pane as the versioned JSON tree of ADR 0036's
// envelope (`render` and `handle-event`, docs/list-tree.md); the SDK's
// adapter (adapt.js) writes the tree and runs the actions, so a command
// never sees the JSON or the callback ids.
/// <reference path="./console.d.ts" />
/// <reference path="./wasi.d.ts" />
/// <reference path="./commands.d.ts" />
/// <reference path="./feedback-host.d.ts" />
/// <reference path="./system-host.d.ts" />
/// <reference path="./system-commands-host.d.ts" />
/// <reference path="./data.d.ts" />
/// <reference path="./operations.d.ts" />
/// <reference path="./applications.d.ts" />
/// <reference path="./helpers.d.ts" />
/// <reference path="./programs-host.d.ts" />
/// <reference path="./files.d.ts" />
/// <reference path="./file-index.d.ts" />
/// <reference path="./typed-folder.d.ts" />
/// <reference path="./clipboard.d.ts" />
/// <reference path="./run.d.ts" />
/// <reference path="./windows.d.ts" />

import type { LaunchRecord } from "pane:extension/commands@0.1.0";
import type { Outcome, Rendered, UiEvent } from "./view.js";

export type {
  ArgumentValue,
  CommandRef,
  LaunchRecord,
  LaunchSource,
  LaunchType,
} from "pane:extension/commands@0.1.0";

/**
 * One entry in a command's list.
 * custom view, else runs its primary action (its first); an item with none
 * of them cannot be activated, and Pane says so.
 */
export interface Item {
  /**
   * Identifies the item among the list's items: Pane keeps the selection on
   * it when the list is drawn again, and passes it to `openView`.
   */
  id: string;
  title: string;
  /** A second line under the title; omitted or `null` for none. */
  subtitle?: string | null;
  /**
   * Runs when the user chooses the item: an untitled action before the
   * item's `actions`, which Pane names "Run item". Pane shows nothing of
   * what it resolves with: it tells the user what happened itself, with a
   * toast or a HUD (`@pane-app/extension/feedback`); throwing shows the error
   * as a failure toast. Pane then asks for the list again (`render`).
   * Omitted or `null` for none.
   */
  onAction?: (() => Promise<void>) | null;
  /**
   * The item's actions, in order (after `onAction`, if it is given): the
   * first is its primary action (Enter), the second its secondary action
   * (Ctrl+Enter), the third runs with Ctrl+Shift+Enter, and the Actions
   * panel (Ctrl+K) lists them all. Omitted or `null` for none.
   */
  actions?: Action[] | null;
  /**
   * The operating systems the item's action works on; omitted or `null`
   * for every system Pane runs on. Elsewhere Pane still lists the item
   * but shows it as unavailable with the reason, and never runs its
   * action.
   */
  platforms?: Platform[] | null;
  /** Drawn before the title (#139); omitted or `null` for none. */
  icon?: Icon | null;
  /** Shown while the pointer rests on the title: all of it, say. */
  titleTooltip?: string | null;
  /** Shown while the pointer rests on the subtitle. */
  subtitleTooltip?: string | null;
  /**
   * On the right of the row, in order: text, a relative date, a coloured
   * tag (#139). A row shows the first three.
   */
  accessories?: Accessory[] | null;
}

import type { Accessory, Icon } from "./icons";
export type {
  Accessory,
  AccessoryOptions,
  Color,
  Icon,
  IconObject,
  IconOptions,
  Tint,
  Tone,
} from "./icons";

/** An operating system Pane runs on. */
export type Platform = "windows" | "macos" | "linux";

/**
 * One of an item's actions, or an entry of a submenu. Choosing it runs
 * `onAction`, which tells the user what happened itself, with a toast or a
 * HUD (`@pane-app/extension/feedback`); throwing shows the error as a failure
 * toast. Pane then asks for the list again (`render`). An action with a
 * `submenu` instead opens that submenu in the Actions panel.
 */
export type Action = CallbackAction | SubmenuAction;

/** An action that runs `onAction` when the user chooses it. */
export interface CallbackAction extends ActionBase {
  onAction: () => Promise<void>;
  submenu?: never;
}

/**
 * An action that opens `submenu` in place in the Actions panel ("Open
 * With…"). As one of an item's actions, Enter, its chord or its shortcut
 * open the panel at it.
 */
export interface SubmenuAction extends ActionBase {
  submenu: Submenu;
  onAction?: never;
}

/**
 * Further choices an action opens in the Actions panel: a title, which the
 * panel shows while it is open, and its entries, each an action of its own.
 * The entries are given with the list (`entries`), or `onOpen` resolves with
 * them each time the user opens the submenu: Pane shows it loading until
 * then, and what `onOpen` throws as its one entry. The panel filters the
 * entries as the user types, and an entry's shortcut works while its
 * submenu is shown.
 */
export type Submenu =
  | { title: string; entries: Action[]; onOpen?: never }
  | { title: string; onOpen: () => Promise<Action[]>; entries?: never };

/** What every action has, whatever choosing it does. */
export interface ActionBase {
  /** What the footer and the Actions panel call it. */
  title: string;
  /**
   * The title of its section in the Actions panel; consecutive actions with
   * the same section are one section. Omitted or `null` for an untitled one.
   */
  section?: string | null;
  /** `"destructive"` draws it in the destructive style. Omitted or `null` for the default. */
  style?: "default" | "destructive" | null;
  /**
   * The keys that run it from the list without opening the Actions panel.
   * Pane matches the modifiers exactly, and never binds one of its own keys
   * (Escape, Ctrl+K, the arrows, Ctrl and a digit, the keys the user gave
   * Pane's actions): the action then stays in the panel without it. Omitted
   * or `null` for none.
   */
  shortcut?: Shortcut | null;
  /**
   * The icon the Actions panel draws beside it in place of Pane's glyph,
   * as an item's icon is drawn (a web image or a system icon shows its
   * fallback until it loaded). Omitted or `null` for Pane's glyph.
   */
  icon?: Icon | null;
}

/**
 * A modifier held with a shortcut's key: `"cmd"` is Command on macOS, the
 * Windows key on Windows and Super on Linux.
 */
export type Modifier = "ctrl" | "alt" | "shift" | "cmd";

/**
 * One key with the modifiers held with it. Keys are named as Pane names
 * them: a letter or digit, a character such as `","`, or `"enter"`,
 * `"delete"`, `"backspace"`, `"up"`, `"f5"` and the like.
 */
export interface ShortcutKeys {
  modifiers: Modifier[];
  key: string;
}

/** One key for every system, or one per system (a system left out binds none). */
export type Shortcut =
  | ShortcutKeys
  | { windows?: ShortcutKeys | null; macos?: ShortcutKeys | null; linux?: ShortcutKeys | null };

/** A command's list view: its title and items, in order. */
export interface List {
  title: string;
  items: Item[];
}


/**

/**
 * A designed view the user has open, holding the view's state: what
 * `createView` makes, or any object with these methods. `openView` returns
 * a new one for each opened view; Pane drops it when the view leaves the
 * screen and never uses it again. The tree is the versioned JSON document
 * of docs/designed-tree.md; `@pane-app/extension/view`'s `createView`
 * writes it, so an author never does.
 */
export interface DesignedView {
  /**
   * Draws the view as it is now. `context` is JSON: the sequence number of
   * this render, so each render's listeners can be named by it, and the
   * version of the UI component set Pane supports. Throwing reports an
   * error to the user; the view keeps its last good tree.
   */
  render(context: string): Promise<Rendered>;
  /**
   * Handle the user's input to the view: `event.callback`, the id the tree
   * named for what was pressed, with the sequence number of the render
   * whose tree the user saw. Pane then calls `render`. The answer may
   * navigate (#239): `push` a view above this one, `replace` this view, or
   * `pop` it with a result. Throwing reports an error to the user; the
   * view stays open.
   */
  handleEvent(event: UiEvent): Promise<Outcome>;
}

/**
 * An extension command, or the commands one component serves. The module
 * exports it as `command`:
 *
 * ```ts
 * export const command: Command = {
 *   async render(launch) {
 *     return {
 *       title: "Hello",
 *       items: [{ id: "greet", title: "Say hello", onAction: async () => { showToast({ title: "Hello" }); } }],
 *     };
 *   },
 * };
 * ```
 *
 * A root provider (`"mode": "provider"` in `pane.json`) only answers root
 * search through its `rootResults` or `indexedResults`: Pane never opens or
 * runs it, so its `command` needs none of these (`export const command:
 * Command = {}`).
 *
 * A no-view command (`"mode": "no-view"` in `pane.json`) has `run` instead
 * of `render`:
 *
 * ```ts
 * export const command: Command = {
 *   async run(id, launch) { showHUD(`Launched from ${launch.source}`); },
 * };
 * ```
 *
 * Resolving gives Pane the value. Throwing (rejecting) reports an error to the
 * user, never a crash: from `render`, `run`, an item's `onAction`,
 * `runSearchResult`, `openView` and a view's `handleEvent` an `Error`'s
 * message, or a thrown string as is. A form is a designed view now
 * (#241), whose submission is an event, so `submitForm` is gone.
 * An action, `run` and `runSearchResult` resolve with nothing: Pane shows
 * nothing of what they resolve with (text is let through and ignored).
 * Resolving with a value of the wrong type, such as an action resolving
 * with `null` or a number, or a provider's `results` resolving with a
 * string, is a crash: Pane reports it and starts a fresh instance for the
 * next call, and repeated crashes pause the extension.
 * A crash closes any open view, whose state was in the old instance.
 * A list Pane cannot read, such as one whose title is not a string, is the
 * command's failure, which Pane reports, not a crash.
 */
export interface Command {
  /**
   * The command's list, as it is now. Pane asks for it when the command
   * opens and again after each action, with `launch`, the launch record the
   * screen was opened with. Without it, opening the command is an error: a
   * no-view command has no list.
   */
  render?(launch: LaunchRecord): Promise<List>;
  /**
   * Runs the no-view command with id `command` (its id in `pane.json`, so
   * one component can serve several commands), launched as `launch` says:
   * how (by the user or in the background, and from where), with any text
   * sent through its alias or as a fallback, and any context another
   * command passed. Pane shows nothing of what it resolves with: it tells
   * the user what happened with a toast or a HUD
   * (`@pane-app/extension/feedback`). Throwing shows the error as a failure
   * toast with a "Copy Error" action, which never counts towards pausing
   * the extension, and a toast left animated is hidden once it ends. Pane
   * calls it only for a command whose `pane.json` entry says `"mode":
   * "no-view"`; without it, that is an error.
   */
  run?(command: string, launch: LaunchRecord): Promise<void>;
  /**
   * Runs the search result with id `id` the user chose, for a command whose
   * List handles its search itself (#240: the List's search-text event, the
   * `command-search` export retired); throwing shows the error as a failure
   * toast. Without it, choosing a result is an error.
   */
  runSearchResult?(id: string): Promise<void>;
  /**
   * Open the designed view of the command with id `command` (its id in
   * `pane.json`, so one component can serve several commands), launched as
   * `launch` says: a screen the extension describes as a tree of layout
   * nodes and UI components that Pane renders (`"mode": "designed"` in
   * `pane.json`, docs/designed-tree.md). `createView` makes the answer:
   *
   * ```ts
   * import { createView } from "@pane-app/extension/view";
   * async openView(command) { return createView(Counter); },
   * ```
   *
   * Throwing reports an error and opens nothing. Without it, opening one is
   * an error. Pane never calls `render` for the command.
   */
  openView?(command: string, launch: LaunchRecord): Promise<DesignedView>;
}

/** What invoking a root result does; Pane performs it. */
export type RootAction =
  | { tag: "copy"; val: string }
  | { tag: "open-url"; val: string }
  | { tag: "open-file"; val: string };

/**
 * One further way to copy a computed answer, as the Actions panel offers
 * it: what the copy is called and the text it copies.
 */
export interface AnswerCopy {
  /** The copy's title, as the panel shows it ("Copy as RGB"). */
  title: string;
  /** The text it copies. */
  text: string;
}

/**
 * What a root result that is an answer's card says beyond its title and
 * action (the calculator's colour and date answers): the section it sits
 * under, the colour of its swatch, and further ways to copy it. An answer
 * without it sits under its command's title and offers only the copy its
 * own action is.
 */
export interface AnswerDetail {
  /** The section the answer is listed under, in place of the command's
   * ("Color", "Date & Time"). */
  section: string;
  /** The colour of the card's swatch, as `#RRGGBB` or `#RRGGBBAA`; null
   * or omitted when the answer is not a colour. */
  swatch?: string | null;
  /** Further ways to copy the answer, each an entry of the Actions
   * panel, in order; Enter's copy stays the result's own action. */
  copies: AnswerCopy[];
}

/**
 * One result computed from root search's query, listed above the results
 * root search finds by title (below them for an `open-file` result).
 */
export interface RootResult {
  /** Identifies the result among this command's results for the query. */
  id: string;
  title: string;
  /** A second line under the title; omitted or `null` for none. */
  subtitle?: string | null;
  /**
   * `{ tag: "copy", val: text }` copies `text` to the clipboard;
   * `{ tag: "open-url", val: url }` opens `url`, an `http://` or `https://`
   * address, with the system's handler for web links (Pane refuses others);
   * `{ tag: "open-file", val: id }` opens the entry with `id`, as the file
   * index's `search()`, `listFolder()` or the typed folder's
   * `listEntries()` gave it, with the system's handler for its type, once
   * Pane has checked it again (Enter on a program the index found shows it
   * in the file manager, never runs it).
   */
  action: RootAction;
  /**
   * What the answer's card shows beyond its title and action, when the
   * result is one (see {@link AnswerDetail}); omitted or `null` for a
   * plain result.
   */
  answer?: AnswerDetail | null;
}

/**
 * Results a command computes from root search's query, such as a
 * calculator's answer (`pane:extension/root-results` in
 * wit/root-results.wit). A command that computes them sets
 * `"rootResults": true` on its entry in `pane.json`, and
 * `"pane": { "rootResults": true }` in its `package.json` so that it is
 * built with the interface; its module exports them as `rootResults`:
 *
 * ```ts
 * export const rootResults: RootResults = {
 *   async resultsFor(query) { return []; },
 * };
 * ```
 *
 * A command whose only job is this, such as the calculator, also says
 * `"mode": "provider"` (a root provider): it has no row of its own, and
 * its answers are root search's only trace of it.
 */
export interface RootResults {
  /**
   * The results for `query`, the text typed into root search, never empty or
   * blank, best first, asked about at `at`. A query the command has no
   * answer for resolves to `[]`: that is not an error. Throwing is the
   * extension failing; Pane lists a result explaining it. Pane asks again
   * on every change of the query; once the query changes or root search is
   * left, it cancels a call still waiting (on `listFolder`, say): the
   * instance is dropped, so its module state is lost, and the next call
   * starts afresh.
   */
  resultsFor(query: string, at: WallTime): Promise<RootResult[]>;
}

/**
 * When Pane asks for results: the moment the user stopped at the query and
 * how far the local time there is from UTC, by the clock root search's own
 * dates are shown by. A command that answers about the current date or
 * time ("now", "today") computes its answer from it; anything else can
 * ignore it.
 */
export interface WallTime {
  /** Milliseconds since the Unix epoch. */
  milliseconds: number;
  /** How far local time at `milliseconds` is from UTC, in milliseconds. */
  offset: number;
}



/**
 * What invoking an indexed result does; Pane performs it.
 * `{ tag: "open-application", val: id }` opens the installed application
 * with `id`, as `open` in `pane:extension/applications@0.1.0` does;
 * `{ tag: "open", val: { target, application } }` opens `target` (a URL of
 * any scheme, a file, a folder or an application) with the system's
 * handler, or with `application`, as `open` in `@pane-app/extension/system`
 * does.
 */
export type IndexedAction =
  | { tag: "open-application"; val: string }
  | { tag: "open"; val: { target: string; application?: string | null } };

/**
 * One root result a command supplies ahead of the query, which root search
 * matches and ranks by title (and alternate titles) like commands.
 */
export interface IndexedResult {
  /** Identifies the result among this command's results. */
  id: string;
  title: string;
  /** A second line under the title; omitted or `null` for none. */
  subtitle?: string | null;
  /**
   * Other titles that find the result, matched as its title is (such as an
   * installed application's `alternateTitles`); the row still shows
   * `title`. Omitted for none.
   */
  alternateTitles?: string[];
  /**
   * Words that find the result, matched as its subtitle is (such as an
   * installed application's `keywords`). Omitted for none.
   */
  keywords?: string[];
  action: IndexedAction;
}

/**
 * Root results a command supplies ahead of the query, such as the installed
 * applications (`pane:extension/indexed-results` in wit/applications.wit).
 * A command that supplies them sets `"indexedResults": true` on its entry in
 * `pane.json`, and `"pane": { "indexedResults": true }` in its
 * `package.json` so that it is built with the interface; its module exports
 * them as `indexedResults`:
 *
 * ```ts
 * export const indexedResults: IndexedResults = {
 *   async results() { return []; },
 * };
 * ```
 *
 * A command whose only job is this, such as Applications, also says
 * `"mode": "provider"` (a root provider): it has no row of its own, and
 * each result it supplies is its own root result.
 */
export interface IndexedResults {
  /**
   * Every result the command supplies, whatever the query. Pane asks once
   * root search is used and again after each return to it, and keeps the
   * answer. Throwing is the extension failing; Pane lists a result
   * explaining it.
   */
  results(): Promise<IndexedResult[]>;
}

/**
 * The operations a package publishes (`pane:extension/published-operations`
 * in wit/operations.wit), served by the component its `pane.json` names
 * under `operations`. That component's `package.json` sets
 * `"pane": { "operations": true }` so that it is built with the interface;
 * its module exports them as `publishedOperations`:
 *
 * ```ts
 * export const publishedOperations: PublishedOperations = {
 *   async runOperation(operation, input) { return "{}"; },
 * };
 * ```
 */
export interface PublishedOperations {
  /**
   * Serve a call to `operation`, which the package's `pane.json` publishes,
   * on behalf of another extension; Pane calls it only for a published
   * operation. `input` and the resolved text are JSON. Throwing reports the
   * operation's own error to the caller: an `Error`'s message, or a thrown
   * string as is.
   */
  runOperation(operation: string, input: string): Promise<string>;
}

/** What one cycle of a continuing service answers. */
export interface Cycle {
  /** The status to show on the command's screen while it is open. */
  status: string;
  /**
   * How long to wait before the next cycle, in seconds: at least 1 and at
   * most 2592000 (30 days); Pane clamps anything else (provisional bounds,
   * as scheduled work's). 0 is not "at once": a service that always asks
   * for the shortest wait runs every second.
   */
  nextSeconds: number;
}

/**
 * A continuing service a command runs while its package's code may run
 * (`pane:extension/service` in wit/service.wit): not on an interval its
 * `pane.json` entry declares, but at the cadence the service itself
 * chooses, cycle by cycle. A command that runs one sets
 * `"service": true` on its entry in `pane.json`, and
 * `"pane": { "service": true }` in its `package.json` so that it is built
 * with the interface; its module exports it as `service`:
 *
 * ```ts
 * export const service: Service = {
 *   async runCycle(command) { return { status: "Watching", nextSeconds: 2 }; },
 * };
 * ```
 */
export interface Service {
  /**
   * Run one cycle of the continuing service of the command with
   * `command` (its id in `pane.json`, so one component can serve several
   * commands' services). Pane calls it only while the package's code may
   * run: from when it is installed (enabled), enabled, replaced by a reload
   * or an update, or Pane starts, until it is disabled, uninstalled, paused
   * after failures or its code is replaced. Each cycle does a slice of the
   * service's work and answers what to show and when to run it again; the
   * module's state lives for that whole time and goes when the generation
   * ends, so the task's state a disable ends is not there when the code
   * runs again. Throwing is an error the service answers with (Pane shows
   * it and runs the next cycle after a second), never a crash; resolving
   * with a value of the wrong type is a crash, and three crashes within
   * five minutes pause the package, which ends the service. Waiting inside
   * the cycle is the service's to do, but it holds every other extension's
   * calls behind it, exactly as an action's wait does.
   */
  runCycle(command: string): Promise<Cycle>;
}
