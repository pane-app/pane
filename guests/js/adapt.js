// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's adapter between a JS/TS command's exports and its component. The
// build (tools/componentize-js/pane_js.py) bundles it into every JS/TS
// component, around the objects the command exports.
//
// It is the SDK's side of ADR 0036's envelope: the command's `render`
// resolves with a list whose items' actions are functions (`onAction`), and
// the adapter answers Pane's `render` with that list as the versioned JSON
// tree of docs/list-tree.md, naming each item's action by the item's id as
// its callback id. Pane's `handle-event` hands such an id back, and the
// adapter runs the function, answering `{"status": text}`. An id the list
// does not name (an instance that has not drawn the list yet asks it first)
// is a search result's, which the command's `runSearchResult` runs. A
// no-view command's `run` answers the same way. Both `render` and `run`
// receive the command's launch record (wit/commands.wit); the adapter
// passes it on, and draws the list again with the last one it was given.
//
// It also makes whatever a handler throws an error it answers with, never a
// crash:
//
// - from `submitForm`, a `FormError`-like object (`{ field?, message }`) as
//   it is, and anything else (an `Error`, a string) as a message about the
//   whole form;
// - from every other handler that answers with an error (`render`, an
//   item's `onAction`, `runSearchResult`, `run`, `openView`, a custom view's
//   `handleEvent`, `resultsFor`, `results`, `runOperation`, `runCycle`),
//   the message of an `Error` or of an object with a `message`, or the text
//   of anything else.
//
// A crash is then only what a crash should be: an action resolving with a
// value that is not text, a provider resolving with a value of the wrong
// type, or a custom view's `render` throwing (it has no error to answer
// with). A list Pane cannot read (a title that is not text, say) is the
// command's failure, which Pane reports, not a crash.

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
 * `list`, the list the command's `render` resolved with, as its tree;
 * its items' actions go into `actions` by callback id, in place of those
 * of the list drawn before.
 */
function tree(list, actions) {
  actions.clear();
  const items = Array.isArray(list?.items)
    ? list.items.map((item) => {
        const node = { id: item?.id, title: item?.title };
        if (item?.subtitle != null) node.subtitle = item.subtitle;
        if (typeof item?.onAction === "function") {
          // The item's id names its action's callback.
          node.actions = [{ onAction: item.id }];
          actions.set(item.id, item.onAction);
        }
        if (item?.form != null) node.form = treeForm(item.form);
        if (item?.platforms != null) node.platforms = item.platforms;
        if (item?.customView != null) node.customView = item.customView;
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

/** What `handle-event` and `run` answer for `status`, the text shown. */
function answer(status) {
  // An action answers text; anything else is a crash, as it was.
  if (typeof status !== "string") return status;
  return JSON.stringify({ status });
}

/**
 * The exported `command`, adapted to Pane's `render`, `handle-event` and
 * `run`.
 */
export function adaptCommand(command) {
  if (command === null || typeof command !== "object") return command;
  const own = { ...missing, ...command };
  const render = adapted(own, "render", message);
  /** The actions of the list the instance drew last, by callback id. */
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
      let status;
      try {
        if (typeof command.run !== "function") {
          throw new Error(`\`${id}\` opens a screen; it has no run entry point`);
        }
        status = await command.run(id, launch);
      } catch (thrown) {
        throw message(thrown);
      }
      return answer(status);
    },
    async handleEvent(callback, _details) {
      if (!actions.has(callback)) {
        // A fresh instance: the list names its actions once drawn.
        await draw(drawnFor);
      }
      const action = actions.get(callback);
      let status;
      try {
        if (action !== undefined) {
          status = await action();
        } else if (typeof command.runSearchResult === "function") {
          status = await command.runSearchResult(callback);
        } else {
          throw new Error(`unknown action: ${callback}`);
        }
      } catch (thrown) {
        throw message(thrown);
      }
      return answer(status);
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
