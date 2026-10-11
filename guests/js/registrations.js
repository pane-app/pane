// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// What an extension registers at run time and owns (`@pane-app/extension/
// registrations`, ADR 0041, #158): a dynamic root item, a timer, a folder
// watcher or a run-time provision, each a handle whose `dispose()` (or
// `using`) undoes it, as the host does when the instance that made it
// goes or the package's code is replaced. Bundled into the command that
// imports it, like any npm module. The underlying functions are also
// importable directly from "pane:extension/registrations@0.1.0" as
// `addRootItem`, `timerAfter`, `timerEvery`, `watchFolder` and
// `provideCapability`, the handles as their resources; the events entry
// point keeps this module's callback table, as the adapter keeps a list's
// (adapt.js).

import * as registrations from "pane:extension/registrations@0.1.0";

export { rootItem, after, every, watchFolder, provide, handleEvent };

/** Where the callbacks of what the component registered are kept, by
 * their tags and callback ids: as the toast's actions are (feedback.js),
 * on globalThis, so the events entry point and the adapter's
 * handle-event find them. Not cleared when a list is drawn, and not
 * taken when they run: what is registered is registered until it is
 * disposed. */
const KEPT = Symbol.for("pane.extension.registrations");

/** The callbacks registered, by tag or callback id: `{ run }` for a
 * timer's or an item action's, `{ watch }` for a watcher's. */
function kept() {
  let map = globalThis[KEPT];
  if (!(map instanceof Map)) {
    map = new Map();
    globalThis[KEPT] = map;
  }
  return map;
}

/** The callback named `id`, if one is registered. */
export function registered(id) {
  const map = globalThis[KEPT];
  return map instanceof Map ? map.get(id) : undefined;
}

/** A fresh tag for a registration's callbacks, never reused, keeping
 * `keep` under it. */
let tagged = 0;
function tag(what, keep) {
  tagged += 1;
  const name = `${what}:${tagged}`;
  kept().set(name, keep);
  return name;
}

/** `item` as the JSON the host takes: the item shape of docs/list-tree.md
 * plus the `mode`, its actions' functions kept under their callback ids. */
function wired(item) {
  const node = { id: item?.id, title: item?.title };
  if (item?.subtitle != null) node.subtitle = item.subtitle;
  const given = Array.isArray(item?.actions) ? item.actions : [];
  if (given.length > 0) {
    // The item's id names its first action's callback, and the id and
    // their place its later ones', as a list item's are.
    node.actions = given.map((action, index) => {
      const callback = index === 0 ? item.id : `${item.id}#${index}`;
      kept().set(callback, { run: action?.onAction });
      const wire = { onAction: callback };
      if (action?.title != null) wire.title = action.title;
      if (action?.section != null) wire.section = action.section;
      if (action?.style != null) wire.style = action.style;
      if (action?.icon != null) wire.icon = action.icon;
      return wire;
    });
  }
  if (item?.mode != null) node.mode = item.mode;
  // Its icon, tooltips and accessories, as a list item's are.
  for (const field of ["icon", "titleTooltip", "subtitleTooltip"]) {
    if (item?.[field] != null) node[field] = item[field];
  }
  if (Array.isArray(item?.accessories)) node.accessories = item.accessories;
  return JSON.stringify(node);
}

/**
 * Registers a dynamic root item under the command with manifest id
 * `command`, from `item`. The handle the host returns is wrapped, so
 * `update` takes an item and `dispose` undoes the registration (the
 * underlying resource is `pane:extension/registrations@0.1.0`'s
 * `RootItem`).
 * @param {string} command
 * @param {import("./registrations.d.ts").Item} item
 * @returns {import("./registrations.d.ts").RootItem}
 */
function rootItem(command, item) {
  const handle = registrations.addRootItem(command, wired(item));
  const drop = () => handle.drop();
  return {
    update(next) {
      handle.update(wired(next));
    },
    dispose: drop,
    [Symbol.dispose]: drop,
  };
}

/**
 * Registers a timer that fires once, `seconds` from now, calling `run`.
 * @param {number} seconds
 * @param {() => (Promise<void> | void)} run
 */
function after(seconds, run) {
  return registrations.timerAfter(seconds, tag("timer", { run }));
}

/**
 * Registers a timer that fires every `seconds`, calling `run`.
 * @param {number} seconds
 * @param {() => (Promise<void> | void)} run
 */
function every(seconds, run) {
  return registrations.timerEvery(seconds, tag("timer", { run }));
}

/**
 * Registers a watcher of `path`, recursive or not, calling `onChanges`
 * with what changed.
 * @param {string} path
 * @param {boolean} recursive
 * @param {(changes: import("./registrations.d.ts").Changes) => (Promise<void> | void)} onChanges
 */
function watchFolder(path, recursive, onChanges) {
  return registrations.watchFolder(path, recursive, tag("watcher", { watch: onChanges }));
}

/**
 * Registers a provision of the capability `capability`.
 * @param {string} capability
 */
function provide(capability) {
  return registrations.provideCapability(capability);
}

/**
 * The events entry point's `handleEvent`, as this module keeps it: the
 * host hands the tag of the timer that fired or the watcher whose
 * changes arrived, and the closure it was registered with runs. A
 * command that registers timers or watchers exports it beside its own,
 * with `"pane": { "events": true }` in its package.json:
 *
 * ```js
 * import * as registrations from "@pane-app/extension/registrations";
 * export const events = { handleEvent: registrations.handleEvent };
 * ```
 *
 * @param {import("./registrations.d.ts").Event} event
 */
async function handleEvent(event) {
  if (event?.tag === "timer") {
    const found = registered(event.val);
    if (found?.run === undefined) {
      throw new Error(`this timer's tag "${event.val}" was not registered`);
    }
    await found.run();
  } else if (event?.tag === "watcher") {
    const watched = event.val;
    const found = registered(watched?.tag);
    if (found?.watch === undefined) {
      throw new Error(`this watcher's tag "${watched?.tag}" was not registered`);
    }
    await found.watch(watched.changes);
  } else {
    throw new Error(`an event this module does not know: ${JSON.stringify(event)}`);
  }
}
