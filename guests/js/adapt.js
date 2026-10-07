// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's adapter between a JS/TS command's exports and its component. The
// build (tools/componentize-js/pane_js.py) bundles it into every JS/TS
// component, around the objects the command exports.
//
// It is the SDK's side of ADR 0036's envelope: the command's `render`
// resolves with a list whose items' actions are functions (an item's
// `onAction`, and each of its `actions`' `onAction`), and the adapter
// answers Pane's `render` with that list as the versioned JSON tree of
// docs/list-tree.md. An item's first action is named by the item's id as
// its callback id, and its later ones by the id and their place (`<id>#1`,
// `<id>#2`, ...); an item's own `onAction` comes before its `actions`.
// Pane's `handle-event` hands such an id back, and the adapter runs the
// function, answering `{}`: Pane shows nothing of an answer, and the
// command tells the user what happened with a toast or a HUD
// (`@pane/extension/feedback`, feedback.js). An id of the newest toast's
// actions (`toast:<n>:primary`) runs that action, from the table
// feedback.js keeps on `globalThis`. An id the list does not name (an
// instance that has not drawn the list yet asks it first) is a search
// result's, which the command's `runSearchResult` runs.
//
// An action may open a `submenu` instead (#140): `{ title, entries }`, its
// entries given with the list and named after the action and their place
// (`<callback>/0`, `<callback>/1`, ...), or `{ title, onOpen }`, a function
// Pane asks for the entries each time the submenu opens: the adapter names
// it as the action would be named, and answers Pane's `handle-event` for it
// with `{"entries": [...]}`, naming the entries the same way. A no-view command's
// `run` answers the same way. Both `render` and `run` receive the command's
// launch record (wit/commands.wit); the adapter passes it on, and draws the
// list again with the last one it was given.
//
// It also makes whatever a handler throws an error it answers with, never a
// crash:
//
// - from `submitForm`, a `FormError`-like object (`{ field?, message }`) as
//   it is, and anything else (an `Error`, a string) as a message about the
//   whole form;
// - from every other handler that answers with an error (`render`, an
//   item's `onAction`, a submenu's `onOpen`, `runSearchResult`, `run`,
//   `openView`, a custom view's `handleEvent`, `resultsFor`, `results`,
//   `runOperation`, `runCycle`), the message of an `Error` or of an object
//   with a `message`, or the text of anything else. A submenu's `onOpen`
//   resolving with something other than a list is such an error too.
//
// A crash is then only what a crash should be: an action, `run` or
// `runSearchResult` resolving with a value (it resolves with nothing; text,
// which Pane no longer shows, is let through), a provider resolving with a
// value of the wrong type, or a custom view's `render` throwing (it has no
// error to answer with). A list Pane cannot read (a title that is not text,
// say) is the command's failure, which Pane reports, not a crash.

import { look } from "./look.js";

/** The version of the tree the adapter writes (docs/list-tree.md). */
const TREE_VERSION = 1;

/** The text of what a handler threw. */
function message(thrown) {
  if (thrown instanceof Error) return thrown.message;
  if (typeof thrown === "string") return thrown;
  if (thrown !== null && typeof thrown === "object" && typeof thrown.message === "string") {
    return thrown.message;
  }
  return String(thrown);
}

/** The form error for what `submitForm` threw. */
function formError(thrown) {
  const plain =
    thrown !== null &&
    typeof thrown === "object" &&
    !(thrown instanceof Error) &&
    typeof thrown.message === "string";
  if (plain && typeof thrown.field === "string") {
    return { field: thrown.field, message: thrown.message };
  }
  return { message: message(thrown) };
}

/**
 * `target[name]`, called on `target`, throwing what `error` makes of what it
 * throws, and passing what it resolves with through `then`.
 */
function adapted(target, name, error, then = (value) => value) {
  const handler = target[name];
  if (typeof handler !== "function") return handler;
  return async (...args) => {
    let value;
    try {
      value = await handler.apply(target, args);
    } catch (thrown) {
      throw error(thrown);
    }
    return then(value);
  };
}

/** `view`, a custom view, with `handleEvent` answering errors as text. */
function adaptView(view) {
  if (view !== null && typeof view === "object" && typeof view.handleEvent === "function") {
    const handleEvent = adapted(view, "handleEvent", message);
    try {
      Object.defineProperty(view, "handleEvent", { value: handleEvent, configurable: true });
    } catch {
      // A frozen view keeps its own handler.
    }
  }
  return view;
}

/** A form as the tree carries it, from the `Form` the command gives. */
function treeForm(form) {
  if (form === null || typeof form !== "object" || !Array.isArray(form.fields)) return form;
  return {
    title: form.title,
    submitLabel: form.submitLabel,
    fields: form.fields.map((field) => {
      const kind = field?.kind;
      const node = { id: field?.id, label: field?.label, kind: kind?.tag };
      if (kind?.tag === "choice") node.choices = kind.val;
      else if (kind?.val?.placeholder != null) node.placeholder = kind.val.placeholder;
      return node;
    }),
  };
}

/**
 * `given`, an item's actions or a submenu's entries, as the tree writes
 * them: each one's callback named `name(place)`, and its function (an
 * action's `onAction`, or a lazy submenu's `onOpen`) kept in `callbacks` by
 * that name. A submenu given with the list names its entries after the
 * action that opens it: `<name>/0`, `<name>/1`, ...
 */
function wireActions(given, name, callbacks) {
  return given.map((action, index) => {
    const callback = name(index);
    const wire = {};
    const submenu = action?.submenu;
    if (submenu !== null && typeof submenu === "object") {
      const node = { title: submenu.title };
      if (typeof submenu.onOpen === "function") {
        node.onOpen = callback;
        callbacks.set(callback, { open: submenu.onOpen });
      } else if (Array.isArray(submenu.entries)) {
        node.entries = wireActions(submenu.entries, (place) => `${callback}/${place}`, callbacks);
      } else {
        node.entries = submenu.entries;
      }
      wire.submenu = node;
    } else {
      wire.onAction = callback;
      if (typeof action?.onAction === "function") callbacks.set(callback, { run: action.onAction });
    }
    if (action?.title != null) wire.title = action.title;
    if (action?.section != null) wire.section = action.section;
    if (action?.style != null) wire.style = action.style;
    if (action?.shortcut != null) wire.shortcut = action.shortcut;
    // Its icon in the Actions panel (#139), written as an item's is.
    if (action?.icon != null) wire.icon = action.icon;
    return wire;
  });
}

/**
 * `list`, the list the command's `render` resolved with, as its tree;
 * its items' actions go into `actions` by callback id, in place of those
 * of the list drawn before (and of the lazy submenus opened since).
 */
function tree(list, actions) {
  actions.clear();
  const items = Array.isArray(list?.items)
    ? list.items.map((item) => {
        const node = { id: item?.id, title: item?.title };
        if (item?.subtitle != null) node.subtitle = item.subtitle;
        const given = [];
        if (typeof item?.onAction === "function") given.push({ onAction: item.onAction });
        if (Array.isArray(item?.actions)) given.push(...item.actions);
        if (given.length > 0) {
          // The item's id names its first action's callback, and the id and
          // their place its later ones'.
          node.actions = wireActions(
            given,
            (index) => (index === 0 ? item.id : `${item.id}#${index}`),
            actions,
          );
        }
        if (item?.form != null) node.form = treeForm(item.form);
        if (item?.platforms != null) node.platforms = item.platforms;
        if (item?.customView != null) node.customView = item.customView;
        // Its icon, tooltips and accessories (#139).
        Object.assign(node, look(item));
        return node;
      })
    : list?.items;
  return JSON.stringify({ version: TREE_VERSION, view: { type: "list", title: list?.title, items } });
}

/** What a command without `render`, `submitForm` or `openView` answers. */
const missing = {
  async render() {
    throw new Error("this command opens no screen");
  },
  async submitForm() {
    throw new Error("this command has no forms");
  },
  async openView() {
    throw new Error("this command has no custom views");
  },
};

/** What `handle-event` and `run` answer: Pane shows nothing of it. */
const ANSWER = "{}";

/**
 * What `handle-event` and `run` answer for `value`, what an action, `run`
 * or `runSearchResult` resolved with: nothing (or text, which Pane no
 * longer shows) answers `{}`; any other value is answered as it is, which
 * is not text, so the call crashes, as it always did.
 */
function answer(value) {
  if (value === undefined || typeof value === "string") return ANSWER;
  return value;
}

/** Where feedback.js keeps the newest toast's actions, by callback id. */
const TOAST_ACTIONS = Symbol.for("pane.extension.toastActions");

/** The newest toast's action named `callback`, if any. */
function toastAction(callback) {
  const actions = globalThis[TOAST_ACTIONS];
  return actions instanceof Map ? actions.get(callback) : undefined;
}

/**
 * The exported `command`, adapted to Pane's `render`, `handle-event` and
 * `run`.
 */
export function adaptCommand(command) {
  if (command === null || typeof command !== "object") return command;
  const own = { ...missing, ...command };
  const render = adapted(own, "render", message);
  /**
   * The callbacks of the list the instance drew last (and of the lazy
   * submenus opened since), by callback id: `{ run }` for an action, `{ open }`
   * for a lazy submenu.
   */
  const actions = new Map();
  /**
   * The launch record the list was last drawn with: a launch by the user
   * from root search before Pane has said.
   */
  let drawnFor = { launchType: "user-initiated", source: "root-search", arguments: [] };
  const draw = async (launch) => {
    drawnFor = launch;
    return tree(await render(launch), actions);
  };
  return {
    ...own,
    render: draw,
    async run(id, launch) {
      let value;
      try {
        if (typeof command.run !== "function") {
          throw new Error(`\`${id}\` opens a screen; it has no run entry point`);
        }
        value = await command.run(id, launch);
      } catch (thrown) {
        throw message(thrown);
      }
      return answer(value);
    },
    async handleEvent(callback, _details) {
      // A toast's action, which stays the toast's while it shows.
      const ofToast = toastAction(callback);
      if (ofToast !== undefined) {
        let value;
        try {
          value = await ofToast();
        } catch (thrown) {
          throw message(thrown);
        }
        return answer(value);
      }
      if (!actions.has(callback)) {
        // A fresh instance: the list names its actions once drawn.
        await draw(drawnFor);
      }
      const found = actions.get(callback);
      let value;
      try {
        if (found?.open !== undefined) {
          // A lazy submenu opens: its entries, named after it.
          const entries = await found.open();
          if (!Array.isArray(entries)) {
            throw new Error("the submenu's onOpen resolved with something other than a list");
          }
          const named = (place) => `${callback}/${place}`;
          return JSON.stringify({ entries: wireActions(entries, named, actions) });
        }
        if (found?.run !== undefined) {
          value = await found.run();
        } else if (typeof command.runSearchResult === "function") {
          value = await command.runSearchResult(callback);
        } else {
          throw new Error(`unknown action: ${callback}`);
        }
      } catch (thrown) {
        throw message(thrown);
      }
      return answer(value);
    },
    submitForm: adapted(own, "submitForm", formError),
    openView: adapted(own, "openView", message, adaptView),
  };
}

/** An exported provider whose handler `name` answers errors as text. */
export function adaptProvider(provider, name) {
  if (provider === null || typeof provider !== "object") return provider;
  return { ...provider, [name]: adapted(provider, name, message) };
}
