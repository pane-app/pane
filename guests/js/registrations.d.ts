// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `@pane-app/extension/registrations` (registrations.js):
// what an extension registers at run time and owns (ADR 0041, #158): a
// dynamic root item, a timer, a folder watcher or a run-time provision,
// each a handle whose `dispose()` (or `using`) undoes it. Undone too when
// the instance that made it goes, or the package's code is replaced: an
// author never writes cleanup code.

/** What a dynamic root item's invocation does when it declares a mode:
 * launch its command, opening its screen (`view`) or running it
 * (`no-view`), with a launch record naming the item's id. */
export type Mode = "view" | "no-view";

/**
 * A dynamic root item, in the item shape of docs/list-tree.md: a row of
 * root search under one of the package's commands, matched and ranked
 * like an indexed result. Its `actions` are repeatable; without a `mode`,
 * invoking the row runs the first one, and with one it launches the
 * command.
 */
export interface Item {
  /** Identifies the item among this component's items. */
  id: string;
  title: string;
  subtitle?: string;
  /** Its icon, tooltips and accessories, as a list item's are. */
  icon?: unknown;
  titleTooltip?: string;
  subtitleTooltip?: string;
  accessories?: unknown[];
  actions?: Action[];
  mode?: Mode;
}

/** An action of a dynamic root item: repeatable, as the item is
 * registered. */
export interface Action {
  title?: string;
  section?: string;
  /** `destructive` draws it red. */
  style?: string;
  icon?: unknown;
  onAction: () => Promise<void> | void;
}

/** One handle of an owned registration: `dispose()` undoes it, as does
 * `using` (Symbol.dispose); an unreachable one is undone when its
 * instance goes, since garbage collection is not deterministic. */
export interface Handle {
  /** Undoes the registration at once. Idempotent: a handle already
   * disposed is not undone again. */
  dispose(): void;
}

/** The handle of a dynamic root item: its row's disappearance is what
 * dropping it does. */
export interface RootItem extends Handle {
  /** Replaces the item this handle owns: the row shows what `item` says,
   * keeping its id. Throws when the item cannot be read or this code of
   * the extension was replaced. */
  update(item: Item): void;
}

/**
 * Registers a dynamic root item under the command with manifest id
 * `command` (its id in `pane.json`, served by this component), from
 * `item`. Throws when `command` is not one of this component's commands,
 * the item cannot be read, the package holds the limit of items, or this
 * code of the extension was replaced.
 * @param {string} command
 * @param {Item} item
 * @returns {RootItem}
 */
export function rootItem(command: string, item: Item): RootItem;

/**
 * Registers a timer that fires once, `seconds` from now (1 to 2592000),
 * calling `run`. Firings that fall due while one is pending, or while the
 * package waits for what it needs, are one firing, delivered when they
 * can be. Throws when `seconds` is beyond the bounds, the package holds
 * the limit of timers, the component exports no events entry point, or
 * this code of the extension was replaced.
 * @param {number} seconds
 * @param {() => Promise<void> | void} run
 * @returns {Handle}
 */
export function after(seconds: number, run: () => Promise<void> | void): Handle;

/**
 * Registers a timer that fires every `seconds` (1 to 2592000) until it is
 * disposed or the generation ends, calling `run`, as `after`'s firings
 * are coalesced. Throws as `after` does.
 * @param {number} seconds
 * @param {() => Promise<void> | void} run
 * @returns {Handle}
 */
export function every(seconds: number, run: () => Promise<void> | void): Handle;

/** One event of what the package registered, as the events entry point
 * receives it: a timer's firing (`tag`) or a watcher's coalesced changes
 * (`val.tag` with `val.changes`). */
export type Event =
  | { tag: "timer"; val: string }
  | { tag: "watcher"; val: { tag: string; changes: Changes } };

/** One change a folder watcher reports, as `onChanges` receives it. */
export type Changes =
  | { tag: "paths"; val: string[] }
  | { tag: "rescan" };

/**
 * Registers a watcher of `path` (a folder the system can watch),
 * recursive or not, calling `onChanges` with what changed, coalesced for
 * half a second, or with a rescan to do after an overflow. None of a
 * waiting package's code runs: its watchers' changes are held and merged
 * into one, delivered when it can run again. Throws when the path cannot
 * be watched, the package holds the limit of watchers, the component
 * exports no events entry point, or this code of the extension was
 * replaced.
 * @param {string} path
 * @param {boolean} recursive
 * @param {(changes: Changes) => Promise<void> | void} onChanges
 * @returns {Handle}
 */
export function watchFolder(
  path: string,
  recursive: boolean,
  onChanges: (changes: Changes) => Promise<void> | void,
): Handle;

/**
 * Registers a provision of the capability `capability`, such as
 * "acme:translate@1": the package provides it while this is held, served
 * by this instance. The package's `pane.json` must declare it under
 * `provides`, marked `atRunTime`. Throws when it does not, the package
 * holds the limit of provisions, or this code of the extension was
 * replaced.
 * @param {string} capability
 * @returns {Handle}
 */
export function provide(capability: string): Handle;

/**
 * The events entry point's `handleEvent`, as this module keeps it: the
 * host hands the tag of the timer that fired or the watcher whose changes
 * arrived, and the closure it was registered with runs. A command that
 * registers timers or watchers exports it, with `"pane": { "events":
 * true }` in its package.json:
 *
 * ```js
 * export const events = { handleEvent: registrations.handleEvent };
 * ```
 * @param {import("./registrations.d.ts").Event} event
 */
export function handleEvent(event: Event): Promise<void>;

/**
 * `pane:extension/events@0.1.0`, as the events entry point receives it:
 * a timer's firing or a watcher's coalesced changes. The command's
 * package.json sets `"pane": { "events": true }` and exports `events =
 * { handleEvent }` (see the registrations sample); `@pane-app/extension`
 * keeps the callbacks, so authors pass them to `every` and
 * `watchFolder` instead.
 */
declare module "pane:extension/events@0.1.0" {
  /** One change of a folder watcher: its tag, and the paths that changed
   * or a rescan. */
  export interface WatcherEvent {
    tag: string;
    changes: { tag: "paths"; val: string[] } | { tag: "rescan" };
  }

  /** One event of what the package registered. */
  export type Event = import("./registrations.d.ts").Event;

  /** Handles `event`: the firing of a timer (its tag) or the coalesced
   * changes of a watcher. An error is the extension's own, logged to its
   * package's extension log; a trap is a crash of the package. */
  export function handleEvent(event: Event): Promise<void>;
}
