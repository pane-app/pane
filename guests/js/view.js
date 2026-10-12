// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// The designed view runtime: JSX elements become the nodes of the tree
// Pane renders (docs/designed-tree.md, ADR 0036), and every UI component
// of the component set — the layout primitives, the shared components,
// Markdown — is a value an author spreads into a layout. A function
// component runs per render, with the state of `useState`, `useRef` and
// `useMemo` kept per instance (by its place in the tree, and its key when
// it gives one), and Pane asks for the tree again after each event, so
// the component reads the state the event's listener changed. A view that
// changes by itself asks for that: `useInterval(ms, run)` runs `run` each
// time `ms` passes while the view is open, and `usePending(load)` presents
// data on its way as loading state (#236) — the arrival asks for a
// drawing itself (#243, `pane:extension/view`), so it is drawn the moment
// it lands, with no timer.
//
// `createView(component)` makes the view a command's `openView` answers
// with: it implements `render(context)` and `handleEvent(event)` for
// Pane's envelope, hiding the wire format and the callback ids. The ids
// name each render's listeners; the tables of the last two renders are
// kept, so a press of what the user could see is delivered even if the
// view has rendered since, and an older one is dropped. React is not
// used: its scheduler needs timers that run only inside calls.
//
// A listener may navigate (#239), returning what `push`, `replace`, `pop`
// or `Push` answer: the pushed or replacing view is a view of the same
// kind, and the `onPop` a push was given runs when the pushed view pops,
// with the result its pop answered. The back key pops a view without
// asking; its pop event carries no result. A root node's
// `navigationTitle` names the view, shown where a screen's title is.
//
// The tree is JSON with the version of the UI component set it targets;
// a Pane that renders another major refuses it naming both versions, and
// one that knows less draws what it understands.

import { askToRender, measureText as witMeasureText } from "pane:extension/view@0.1.0";

/** The version of the UI component set this SDK writes. */
const COMPONENT_SET = "1.3";

/** The empty outcome: what a handler that navigates nowhere answers. */
const NOTHING_NEXT = { push: null, replace: null, pop: null };

/** The callback id of the pop event, the event Pane sends a view when the
 * one above it popped: an id no tree names (this SDK's ids start at 1),
 * its payload `{"pop": <result>}`, the result the pop answered (`null`
 * when the back key popped, so no view's result reached the one below). */
const POP_CALLBACK = 0;

/**
 * A built-in component of the tree, named `name`: a function only so JSX
 * and TypeScript accept it as a component; the renderer reads the `node`
 * property instead of calling it.
 */
function builtin(name) {
  const component = () => {
    throw new Error(`the ${name} component is drawn by the tree runtime, never called`);
  };
  component.node = name;
  return component;
}

/** A column: children below each other. */
export const Column = builtin("column");
/** A row: children beside each other. */
export const Row = builtin("row");
/** Children drawn over each other, each placed. */
export const Stack = builtin("stack");
/** A scrolling region. */
export const Scroll = builtin("scroll");
/** Space: it grows to fill what it is given. */
export const Spacer = builtin("spacer");
/** A hairline rule. */
export const Divider = builtin("divider");
/** One line of text, its children spelling it or its spans. */
export const Text = builtin("text");
/** One styled or linked span of a text's content. */
export const Span = builtin("span");
/** A button, pressed by its `onClick`. */
export const Button = builtin("button");
/** A link, pressed by its `onClick`. */
export const Link = builtin("link");
/** One icon, by name, file, URL or system reference. */
export const Icon = builtin("icon");
/** One icon on Pane's tile. */
export const IconTile = builtin("icon-tile");
/** One image, its children standing in for it while it loads. */
export const Image = builtin("image");
/** A rich row: a title, a subtitle, an icon and accessories. */
export const RichRow = builtin("rich-row");
/** One keycap: the key its cap shows. */
export const Keycap = builtin("keycap");
/** A key sequence: the keys its caps show. */
export const KeySequence = builtin("key-sequence");
/** One tag: a short label in a chip. */
export const Tag = builtin("tag");
/** One badge: a short count in a filled chip. */
export const Badge = builtin("badge");
/** One toggle: on or off, changed by its `onChange`. */
export const Toggle = builtin("toggle");
/** One checkbox: checked or not, changed by its `onChange`. */
export const Checkbox = builtin("checkbox");
/** A segmented control, its choice changed by its `onChange`. */
export const Segmented = builtin("segmented");
/** One slider, adjusted by its `onChange`. */
export const Slider = builtin("slider");
/** One progress bar. */
export const Progress = builtin("progress");
/** One loading indicator. */
export const Loading = builtin("loading");
/** Markdown, its children spelling its source. */
export const Markdown = builtin("markdown");
/** A card of children, on Pane's own card surface. */
export const Card = builtin("card");
/** A section header: a title over a group. */
export const SectionHeader = builtin("section-header");
/** A metadata list: rows of a label and its value. */
export const MetadataList = builtin("metadata-list");
/** An empty state: an icon, a title and a description. */
export const EmptyState = builtin("empty-state");
/** One text input. */
export const TextInput = builtin("text-input");
/** One password field. */
export const PasswordInput = builtin("password-input");
/** One text area. */
export const TextArea = builtin("text-area");
/** A select, its choice changed by its `onChange`. */
export const Select = builtin("select");
/** A standard List (#240): items in sections, its search field and
 * selection Pane's. Its children are `List.Section` and `List.Item` (or
 * `Grid.Item` for the Grid), a `List.Dropdown` and an `EmptyView`. */
export const List = builtin("list");
/** A standard Grid (#240): a List's behaviour with cells. */
export const Grid = builtin("grid");
/** A section of a List's or Grid's items (see `List.Section`). */
export const Section = builtin("list-section");
/** One item of a List (see `List.Item`). */
export const Item = builtin("list-item");
/** One cell of a Grid (see `Grid.Item`). */
export const Cell = builtin("grid-item");
/** A List's search-bar dropdown (see `List.Dropdown`). */
export const Dropdown = builtin("list-dropdown");
/** A Detail (#240): a scrolled column of Markdown, metadata, actions. */
export const Detail = builtin("detail");

// The Raycast-style nesting: List.Section, List.Item, List.Dropdown and
// Grid.Item are the components themselves.
List.Section = Section;
List.Item = Item;
List.Dropdown = Dropdown;
Grid.Section = Section;
Grid.Item = Cell;
/** A canvas: a leaf the view draws into by its `ops`, taking input. */
export const Canvas = builtin("canvas");

/**
 * A form (#241): its children are the author's own layout, the fields
 * anywhere in it, and its submission an action — `onSubmit` runs with
 * the values the form was submitted with, keyed by the fields' `id`s;
 * `submitTitle` names its submit button. Enter in a single-line field
 * submits, as Ctrl+Enter does in a text area; a submission's validation
 * errors are the fields' `error` in the tree the view answers with.
 */
export const Form = builtin("form");
/** A form's text field. */
Form.TextField = builtin("text-input");
/** A form's password field. */
Form.PasswordField = builtin("password-input");
/** A form's text area, its Enter a newline and Ctrl+Enter the submit. */
Form.TextArea = builtin("text-area");
/** A form's checkbox. */
Form.Checkbox = builtin("checkbox");
/** A form's toggle. */
Form.Toggle = builtin("toggle");
/** A form's date field, "YYYY-MM-DD". */
Form.DatePicker = builtin("date-picker");
/** A form's date and time field, "YYYY-MM-DD HH:MM". */
Form.DateTimePicker = builtin("date-time-picker");
/** A form's dropdown, its options searched as the user types. */
Form.Dropdown = builtin("select");
/** A form's tag picker: several of its options, chosen as chips. */
Form.TagPicker = builtin("tag-picker");
/** A form's file picker: a path typed or chosen with the system's dialog. */
Form.FilePicker = builtin("file-picker");
/** A form's folder picker. */
Form.FolderPicker = builtin("folder-picker");
/** A form's description, its children its Markdown source. */
Form.Description = builtin("markdown");
/** A form's separator: a hairline rule. */
Form.Separator = builtin("divider");
/** A form's link. */
Form.Link = builtin("link");

/** A fragment: its children are drawn where it sits, unwrapped. */
export const Fragment = Symbol.for("pane.extension.fragment");

/** The selected item's key of the List or Grid the view renders, as its
 * render context names it (#240): the item the detail pane's content is
 * built for. `null` when no item is selected; read during a render. */
let selected = null;

/**
 * The selected item's key of the view's List or Grid, as this render's
 * context names it: the item the detail pane's content is built for
 * (`null` when no item is selected). Read during a render.
 */
export function selectedKey() {
  return selected;
}

/** The render in progress, where the hooks write; `null` outside one. */
let rendering = null;

/** The ticks and pending data this render registers (see `useInterval`
 * and `usePending`), with the view's id, for what asks to be drawn again;
 * `null` outside one. */
let collecting = null;

/**
 * State the component keeps between renders: `[value, set]`. `set` stores
 * the next value (a function is called with the current one); Pane asks
 * for the tree again after each event, which is when it is read.
 */
export function useState(initial) {
  const host = rendering ?? throwOutside("useState");
  const index = host.cursor++;
  if (index === host.cells.length) {
    host.cells.push(typeof initial === "function" ? initial() : initial);
  }
  const cells = host.cells;
  const set = (next) => {
    cells[index] = typeof next === "function" ? next(cells[index]) : next;
  };
  return [cells[index], set];
}

/** A value the component keeps between renders, mutable in place. */
export function useRef(initial) {
  const host = rendering ?? throwOutside("useRef");
  const index = host.cursor++;
  if (index === host.cells.length) {
    host.cells.push({ current: initial });
  }
  return host.cells[index];
}

/** A value computed once, recomputed when `deps` changes. */
export function useMemo(make, deps) {
  const host = rendering ?? throwOutside("useMemo");
  const index = host.cursor++;
  if (index === host.cells.length) {
    host.cells.push({ deps: deps ?? [], value: make() });
  }
  const cell = host.cells[index];
  if (!sameDeps(cell.deps, deps)) {
    cell.deps = deps ?? [];
    cell.value = make();
  }
  return cell.value;
}

/**
 * Runs `run` once each time `ms` passes while the view is open
 * (`useInterval(1000, ...)`, #236): Pane asks for the tree again after
 * `ms`, and the run happens in that render, before the tree is drawn, so
 * the tree shows what it changed. The view's clock is Pane's: an interval
 * is a request to be drawn again, so the time a render takes, or a hidden
 * view, delays the next run — which the next drawing then serves, once,
 * rather than replaying the runs that passed.
 *
 * Each render registers the run afresh, so the interval always runs the
 * listener the newest render named, and a view that stops rendering an
 * interval stops asking for it.
 */
export function useInterval(ms, run) {
  if (rendering === null) throwOutside("useInterval");
  collecting.ticks.push({ ms, run });
}

/**
 * Data the view is waiting for, as ordinary loading state (#236):
 * `usePending(load)` answers `undefined` while the work `load` started has
 * not answered — render a loading state for that — and its answer once it
 * has. The first render starts the work and answers `undefined`, so the
 * loading state is shown at once; when the work answers, the SDK itself
 * asks Pane to draw the view again (#243, `pane:extension/view`), and
 * that drawing shows the answer — the moment it arrived, with no timer to
 * wait for. Pane keeps the instance running between calls while a view of
 * its is open, so the work is awaited in the background; it is still
 * bounded by the call's limits, as any guest work is.
 *
 * The work runs once; a view that wants it again renders another one.
 */
export function usePending(load) {
  const host = rendering ?? throwOutside("usePending");
  const index = host.cursor++;
  if (index === host.cells.length) {
    // The view whose render started the work: the one that asks to be
    // drawn again when it answers, however many views the component's
    // command opens.
    const view = collecting.view;
    const cell = { value: undefined, work: null };
    cell.work = Promise.resolve()
      .then(load)
      .then((answer) => {
        cell.value = answer;
        askToRender(view);
      });
    host.cells.push(cell);
  }
  return host.cells[index].value;
}

/** Throws the "outside a render" error a hook's host is missing. */
function throwOutside(name) {
  throw new Error(`${name} was called outside a component's render`);
}

/** Whether two dependency lists are the same, shallowly. */
function sameDeps(a, b) {
  const other = b ?? [];
  return a.length === other.length && a.every((dep, index) => dep === other[index]);
}

/**
 * Runs `component` with `props`, its hook state kept in `cells` at `path`:
 * state the component had last render is still there, in hook order.
 */
function runComponent(component, props, cells, path, used) {
  let own = cells.get(path);
  if (own === undefined) {
    own = [];
    cells.set(path, own);
  }
  used.add(path);
  const previous = rendering;
  rendering = { cells: own, cursor: 0 };
  try {
    return component(props ?? {});
  } finally {
    rendering = previous;
  }
}

/**
 * `value`, an element or a list of them, as nodes appended to `into`:
 * each child's place under `parentPath` is its key, else its index, so
 * component instances keep their state across renders (as React's do:
 * keys keep a reordered instance's state, positions share one without).
 */
function write(value, parentPath, callbacks, cells, used, into) {
  if (value === null || value === undefined || typeof value === "boolean") {
    return;
  }
  if (typeof value === "string" || typeof value === "number") {
    into.push({ type: "text", text: String(value) });
    return;
  }
  if (Array.isArray(value)) {
    value.forEach((child, index) =>
      writeChild(child, index, parentPath, callbacks, cells, used, into),
    );
    return;
  }
  writeChild(value, 0, parentPath, callbacks, cells, used, into);
}

/** One child at `index` under `parentPath`, as a node in `into`. */
function writeChild(child, index, parentPath, callbacks, cells, used, into) {
  if (child === null || child === undefined || typeof child === "boolean") {
    return;
  }
  if (typeof child === "string" || typeof child === "number") {
    into.push({ type: "text", text: String(child) });
    return;
  }
  if (Array.isArray(child)) {
    write(child, parentPath, callbacks, cells, used, into);
    return;
  }
  const at = child.key === undefined ? String(index) : String(child.key);
  const path = `${parentPath}/${at}`;
  const { type, props } = child;
  if (type === Fragment) {
    write((props ?? {}).children, path, callbacks, cells, used, into);
    return;
  }
  if (typeof type === "function") {
    if (typeof type.node === "string") {
      into.push(builtinNode(type.node, props, path, callbacks, cells, used));
      return;
    }
    // A function component: run it and write what it rendered, its
    // children's places under its own.
    const rendered = runComponent(type, props, cells, path, used);
    write(rendered, path, callbacks, cells, used, into);
    return;
  }
  throw new Error(`a tree's element type must be a component, not ${typeof type}`);
}

/** The listener `listener` becomes, told the text the event's payload
 * names: its `value` (a field's, a control's) or `key` (a key pressed),
 * which the listeners that take no argument ignore. */
function textListener(callbacks, listener) {
  const id = callbacks.size + 1;
  callbacks.set(id, (payload) => listener(payloadText(payload)));
  return id;
}

/** The listener `listener` becomes, told what the event's payload carries
 * as its value, whatever type it is: a string, a boolean, or the list a
 * picker chose; the values a form was submitted with, keyed by the
 * fields' ids (#241). */
function valueListener(callbacks, listener) {
  const id = callbacks.size + 1;
  callbacks.set(id, (payload) => listener(payloadValue(payload)));
  return id;
}

/** One built-in node, with the properties `props` gives it. */
function builtinNode(name, props, path, callbacks, cells, used) {
  const given = props ?? {};
  const children = [];
  // A dropdown's children are its items, given as objects rather than
  // elements; every other component's are written as nodes.
  if (name !== "list-dropdown") {
    write(given.children, path, callbacks, cells, used, children);
  }
  const node = { type: name };
  if (given.key !== undefined) node.key = given.key;
  // A form field's `id` is its key: the identity its value is collected
  // under and its state kept by (#241).
  if (given.id !== undefined && node.key === undefined) node.key = given.id;
  // What a form field says around its control, and asks for: its title,
  // note, error, remembered value and the keyboard (#241).
  if (given.title !== undefined) node.title = given.title;
  if (given.info !== undefined) node.info = given.info;
  if (given.error !== undefined) node.error = given.error;
  if (given.remember === true) node.remember = true;
  if (given.autoFocus === true) node.focus = true;
  if (given.name !== undefined) node.name = given.name;
  if (given.navigationTitle !== undefined) node.navigationTitle = given.navigationTitle;
  if (given.requires !== undefined) node.requires = given.requires;
  // The keyboard asks and events a focusable node names: the focus ask
  // (honoured when it is new, so an open's asks and a later one from code
  // both focus the node), and the focus, blur and key handlers.
  if (given.focus === true) node.focus = true;
  if (typeof given.onFocus === "function") {
    node.onFocus = textListener(callbacks, given.onFocus);
  }
  if (typeof given.onBlur === "function") {
    node.onBlur = textListener(callbacks, given.onBlur);
  }
  if (typeof given.onKey === "function") {
    node.onKey = textListener(callbacks, given.onKey);
  }
  if (given.fallback != null) {
    // The fallback is drawn in the node's place when Pane does not know
    // it, with its own children under the node's place.
    const fallback = [];
    write(given.fallback, path, callbacks, cells, used, fallback);
    node.fallback = fallback[0];
  }
  // The style every node carries: its sizing and its surface, with its
  // hover and pressed variants, and its place in a stack.
  writeStyle(node, given);
  switch (name) {
    case "column":
    case "row":
    case "card": {
      for (const field of ["gap", "padding", "align", "justify"]) {
        if (given[field] !== undefined) node[field] = given[field];
      }
      if (given.wrap !== undefined) node.wrap = given.wrap;
      node.children = children;
      return node;
    }
    case "stack": {
      if (given.align !== undefined) node.align = given.align;
      node.children = children;
      return node;
    }
    case "scroll": {
      if (given.orientation !== undefined) node.orientation = given.orientation;
      node.children = children;
      return node;
    }
    case "divider": {
      if (given.orientation !== undefined) node.orientation = given.orientation;
      return node;
    }
    case "spacer": {
      return node;
    }
    case "text": {
      // The children spell the text, or its spans: a Span element among
      // them makes the text one of spans.
      const spans = [];
      const plain = [];
      let linked = false;
      for (const child of asList(given.children)) {
        if (isElement(child) && child.type === Span) {
          linked = true;
          spans.push(spanNode(child, path, callbacks));
        } else if (linked) {
          spans.push(spanNode(child, path, callbacks));
        } else {
          plain.push(child);
        }
      }
      if (linked) {
        node.spans = spans;
      } else {
        node.text = textOf(given.children);
      }
      if (given.style !== undefined) node.style = given.style;
      if (given.level !== undefined) node.level = given.level;
      if (given.color !== undefined) node.color = given.color;
      if (given.size !== undefined) node.size = given.size;
      if (given.weight !== undefined) node.weight = given.weight;
      if (given.truncate !== undefined) node.truncate = given.truncate;
      return node;
    }
    case "button": {
      node.label = textOf(given.children);
      if (given.tone !== undefined) node.tone = given.tone;
      if (given.icon !== undefined) node.icon = given.icon;
      if (given.keys !== undefined) node.keys = given.keys;
      if (given.enabled !== undefined) node.enabled = given.enabled;
      if (typeof given.onClick === "function") {
        node.onPress = textListener(callbacks, given.onClick);
      }
      return node;
    }
    case "link": {
      node.label = textOf(given.children);
      if (given.color !== undefined) node.color = given.color;
      if (typeof given.onClick === "function") {
        node.onPress = textListener(callbacks, given.onClick);
      }
      return node;
    }
    case "icon":
    case "icon-tile": {
      node.icon = given.icon ?? {};
      if (given.size !== undefined) node.size = given.size;
      return node;
    }
    case "image": {
      node.image = given.image ?? {};
      if (given.size !== undefined) node.size = given.size;
      if (given.fit !== undefined) node.fit = given.fit;
      node.children = children;
      return node;
    }
    case "rich-row": {
      node.title = textOf(given.children);
      if (given.subtitle !== undefined) node.subtitle = given.subtitle;
      if (given.icon !== undefined) node.icon = given.icon;
      if (given.accessories !== undefined) node.accessories = given.accessories;
      if (typeof given.onClick === "function") {
        node.onPress = textListener(callbacks, given.onClick);
      }
      return node;
    }
    case "keycap": {
      node.key = textOf(given.children);
      return node;
    }
    case "key-sequence": {
      node.keys = given.keys ?? [];
      return node;
    }
    case "tag":
    case "badge": {
      node.text = textOf(given.children);
      if (given.color !== undefined) node.color = given.color;
      return node;
    }
    case "toggle":
    case "checkbox": {
      if (name === "toggle") {
        node.on = given.on === true || given.defaultValue === true;
      } else {
        node.checked = given.on === true || given.defaultValue === true;
      }
      if (given.label !== undefined) node.label = given.label;
      if (typeof given.onChange === "function") {
        node.onChange = valueListener(callbacks, given.onChange);
      }
      return node;
    }
    case "segmented":
    case "select": {
      node.options = given.options ?? [];
      if (given.value !== undefined) node.value = given.value;
      if (given.defaultValue !== undefined) node.default = given.defaultValue;
      if (given.placeholder !== undefined) node.placeholder = given.placeholder;
      if (given.label !== undefined) node.label = given.label;
      if (typeof given.onChange === "function") {
        node.onChange = textListener(callbacks, given.onChange);
      }
      // A dropdown whose search the extension handles: the query is told
      // to it as the user types, and the popup shows the options the view
      // answers with, filtering none of its own (#241).
      if (typeof given.onSearchText === "function") {
        node.onInput = textListener(callbacks, given.onSearchText);
        node.search = false;
      }
      return node;
    }
    case "slider": {
      node.value = given.value ?? 0;
      if (given.min !== undefined) node.min = given.min;
      if (given.max !== undefined) node.max = given.max;
      if (given.step !== undefined) node.step = given.step;
      if (given.label !== undefined) node.label = given.label;
      if (typeof given.onChange === "function") {
        node.onChange = textListener(callbacks, given.onChange);
      }
      return node;
    }
    case "progress": {
      node.value = given.value ?? 0;
      if (given.label !== undefined) node.label = given.label;
      return node;
    }
    case "loading": {
      if (given.label !== undefined) node.label = given.label;
      return node;
    }
    case "markdown": {
      node.markdown = textOf(given.children);
      return node;
    }
    case "section-header": {
      node.title = textOf(given.children);
      if (given.note !== undefined) node.note = given.note;
      return node;
    }
    case "metadata-list": {
      node.items = given.items ?? [];
      return node;
    }
    case "empty-state": {
      if (given.title !== undefined) node.title = given.title;
      if (given.description !== undefined) node.description = given.description;
      if (given.icon !== undefined) node.icon = given.icon;
      node.children = children;
      return node;
    }
    case "list":
    case "grid": {
      for (const field of [
        "searchPlaceholder",
        "searchText",
        "selectedKey",
        "isLoading",
        "isShowingDetail",
        "hasMore",
        "pageSize",
      ]) {
        if (given[field] !== undefined) node[field] = given[field];
      }
      if (typeof given.onSearchText === "function") {
        node.onSearchText = textListener(callbacks, given.onSearchText);
      }
      if (typeof given.onSelectionChange === "function") {
        node.onSelectionChange = textListener(callbacks, given.onSelectionChange);
      }
      if (typeof given.onLoadMore === "function") {
        const id = callbacks.size + 1;
        callbacks.set(id, given.onLoadMore);
        node.onLoadMore = id;
      }
      node.children = children;
      return node;
    }
    case "list-section": {
      if (given.title !== undefined) node.title = given.title;
      if (given.subtitle !== undefined) node.subtitle = given.subtitle;
      if (given.columns !== undefined) node.columns = given.columns;
      if (given.aspectRatio !== undefined) node.aspectRatio = given.aspectRatio;
      if (given.fit !== undefined) node.fit = given.fit;
      if (given.inset !== undefined) node.inset = given.inset;
      node.children = children;
      return node;
    }
    case "list-item": {
      node.title = textOf(given.children);
      if (given.title !== undefined) node.title = given.title;
      if (given.subtitle !== undefined) node.subtitle = given.subtitle;
      if (given.icon !== undefined) node.icon = given.icon;
      if (given.accessories !== undefined) node.accessories = given.accessories;
      if (given.keywords !== undefined) node.keywords = given.keywords;
      if (typeof given.onClick === "function") {
        const id = callbacks.size + 1;
        callbacks.set(id, given.onClick);
        node.onPress = id;
      }
      if (Array.isArray(given.actions)) {
        node.actions = given.actions.map((action) => {
          const held = { onPress: 0 };
          if (action.title !== undefined) held.title = action.title;
          if (typeof action.onClick === "function") {
            const id = callbacks.size + 1;
            callbacks.set(id, action.onClick);
            held.onPress = id;
          }
          return held;
        });
      }
      if (given.detail != null) {
        const detail = [];
        write(given.detail, path, callbacks, cells, used, detail);
        node.detail = detail[0];
      }
      node.children = children;
      return node;
    }
    case "grid-item": {
      if (given.title !== undefined) node.title = given.title;
      if (given.subtitle !== undefined) node.subtitle = given.subtitle;
      if (given.image !== undefined) node.image = given.image;
      if (given.color !== undefined) node.color = given.color;
      if (typeof given.onClick === "function") {
        const id = callbacks.size + 1;
        callbacks.set(id, given.onClick);
        node.onPress = id;
      }
      node.children = children;
      return node;
    }
    case "list-dropdown": {
      if (given.value !== undefined) node.value = given.value;
      if (given.placeholder !== undefined) node.placeholder = given.placeholder;
      if (typeof given.onChange === "function") {
        node.onChange = valueListener(callbacks, given.onChange);
      }
      node.items = asList(given.children).map((child) => {
        const held = isElement(child) ? (child.props ?? {}) : child ?? {};
        return {
          value: held.value ?? textOf(held.children),
          ...(held.title !== undefined ? { title: held.title } : {}),
          ...(held.label !== undefined && held.title === undefined ? { title: held.label } : {}),
        };
      });
      return node;
    }
    case "detail": {
      node.children = children;
      return node;
    }
    case "text-input":
    case "password-input":
    case "text-area": {
      node.value = given.value !== undefined ? String(given.value) : textOf(given.children);
      if (given.defaultValue !== undefined) node.default = String(given.defaultValue);
      if (given.placeholder !== undefined) node.placeholder = given.placeholder;
      if (given.label !== undefined) node.label = given.label;
      // The field is partially controlled: `onInput` hears its value as
      // the user types it (coalesced by Pane, throttled by `throttleMs`),
      // `onChange` on its commits, and a `value` that differs from the
      // field's value in the previous render replaces its text.
      if (typeof given.onInput === "function") {
        node.onInput = textListener(callbacks, given.onInput);
      }
      if (typeof given.onChange === "function") {
        node.onChange = textListener(callbacks, given.onChange);
      }
      if (given.throttleMs !== undefined) node.throttleMs = given.throttleMs;
      return node;
    }
    case "canvas": {
      if (given.ops !== undefined) node.ops = given.ops;
      if (given.role !== undefined) node.role = given.role;
      if (given.label !== undefined) node.label = given.label;
      if (given.value !== undefined) node.value = given.value;
      // The canvas's input: each handler is told the event that reached
      // it, its payload read into what it names. The semantic handlers
      // (the arrows and Space) are plain listeners, as a button's are.
      for (const [field, name] of [
        ["onPointerDown", "onPointerDown"],
        ["onPointerUp", "onPointerUp"],
        ["onPointerMove", "onPointerMove"],
        ["onPointerEnter", "onPointerEnter"],
        ["onPointerLeave", "onPointerLeave"],
        ["onWheel", "onWheel"],
        ["onDoubleClick", "onDoubleClick"],
        ["onSecondary", "onSecondary"],
        ["onResize", "onResize"],
      ]) {
        if (typeof given[field] === "function") {
          const id = callbacks.size + 1;
          callbacks.set(id, (payload) => given[field](canvasEvent(payload)));
          node[name] = id;
        }
      }
      for (const field of ["onIncrement", "onDecrement", "onActivate"]) {
        if (typeof given[field] === "function") {
          const id = callbacks.size + 1;
          callbacks.set(id, (payload) => given[field]());
          node[field] = id;
        }
      }
      return node;
    }
    case "form": {
      if (given.submitTitle !== undefined) node.submitLabel = given.submitTitle;
      if (typeof given.onSubmit === "function") {
        node.onSubmit = valueListener(callbacks, given.onSubmit);
      }
      return node;
    }
    case "date-picker":
    case "date-time-picker": {
      node.value = given.value !== undefined ? String(given.value) : "";
      if (given.defaultValue !== undefined) node.default = String(given.defaultValue);
      if (given.placeholder !== undefined) node.placeholder = given.placeholder;
      if (typeof given.onChange === "function") {
        node.onChange = valueListener(callbacks, given.onChange);
      }
      return node;
    }
    case "tag-picker": {
      node.tags = Array.isArray(given.tags)
        ? given.tags.map(String)
        : Array.isArray(given.defaultValue)
          ? given.defaultValue.map(String)
          : [];
      node.options = given.options ?? [];
      if (typeof given.onChange === "function") {
        node.onChange = valueListener(callbacks, given.onChange);
      }
      return node;
    }
    case "file-picker":
    case "folder-picker": {
      if (Array.isArray(given.value)) node.value = given.value.map(String);
      else if (given.value !== undefined) node.value = [String(given.value)];
      else if (Array.isArray(given.defaultValue)) node.value = given.defaultValue.map(String);
      else if (given.defaultValue !== undefined) node.value = [String(given.defaultValue)];
      if (given.allowMultiple === true) node.multiple = true;
      if (typeof given.onChange === "function") {
        node.onChange = valueListener(callbacks, given.onChange);
      }
      return node;
    }
    default:
      throw new Error(`the tree has no ${name} component`);
  }
}

/** The canvas event `payload` names, read into what it names: the event
 * kind, the point in the canvas's own space, and what else it carries. A
 * payload that does not name one answers `null`. */
function canvasEvent(payload) {
  try {
    const held = JSON.parse(payload);
    if (typeof held?.event !== "string") return null;
    return held;
  } catch {
    return null;
  }
}

/** The style properties every node carries, written from `given`. */
function writeStyle(node, given) {
  for (const field of [
    "grow",
    "shrink",
    "basis",
    "width",
    "height",
    "minWidth",
    "maxWidth",
    "minHeight",
    "maxHeight",
    "aspectRatio",
    "background",
    "border",
    "radius",
    "opacity",
    "place",
  ]) {
    if (given[field] !== undefined) node[field] = given[field];
  }
  if (given.offset !== undefined) node.offset = given.offset;
  if (given.hover !== undefined) node.hover = given.hover;
  if (given.pressed !== undefined) node.pressed = given.pressed;
}

/** One span of a text's content, from `child` — a Span element or plain
 * text — with the listener its `onClick` names. */
function spanNode(child, path, callbacks) {
  const given = isElement(child) ? (child.props ?? {}) : { children: child };
  const span = { text: textOf(given.children) };
  for (const field of ["style", "level", "color"]) {
    if (given[field] !== undefined) span[field] = given[field];
  }
  if (given.code === true) span.code = true;
  if (typeof given.onClick === "function") {
    span.onPress = textListener(callbacks, given.onClick);
  }
  return span;
}

/** Whether `value` is an element the tree draws. */
function isElement(value) {
  return value !== null && typeof value === "object" && typeof value.type !== "undefined";
}

/** `value` as the list of children it is. */
function asList(children) {
  if (children === null || children === undefined) return [];
  if (Array.isArray(children)) return children.flat();
  return [children];
}

/** The text `children` spell: strings and numbers, joined. */
function textOf(children) {
  if (children === null || children === undefined) return "";
  if (typeof children === "string") return children;
  if (typeof children === "number") return String(children);
  if (Array.isArray(children)) return children.map(textOf).join("");
  throw new Error("a text's children must be strings or numbers");
}

/**
 * The view a command's `openView` answers with: `component` rendered as
 * its tree, its state kept across renders, its listeners called by the
 * events Pane sends. `props`, when given, are passed to the component.
 *
 * A listener may navigate, returning what [`push`], [`replace`], [`pop`]
 * or [`Push`] answer (or an object of the same shape): the pushed or
 * replacing view is a `createView` view too, and the `onPop` a push was
 * given runs when the pushed view pops, with the result its pop
 * answered, the view re-rendering after.
 */
export function createView(component, props = {}) {
  /** The hook cells of each component instance, by its place in the tree. */
  const cells = new Map();
  /** The listeners of the last two renders, by render number and id. */
  const tables = new Map();
  /** The `onPop` the view's last push registered, run when it pops. */
  let onPop = null;
  /** The ticks the last render registered. */
  let registered = { ticks: [] };
  /** The newest render asked, to number a context that names none. */
  let newest = 0;
  /** The view's id, as its render context names it: what asks Pane to
   * draw it again when pending data lands (#243). */
  let view = 0;
  return {
    async render(context) {
      const number = numberOf(context, newest + 1);
      newest = Math.max(newest, number);
      view = viewIdOf(context) || view;
      /** The render's listeners, by the ids its tree names. */
      const callbacks = new Map();
      /** The component places this render visited, to drop the rest. */
      const used = new Set();
      // This render answers the refresh the view asked for — the drawing
      // Pane waited to ask for, as the context names — which is where an
      // interval's work runs, so the tree shows what it changed. Pending
      // data is never awaited here: it asks for a drawing of its own when
      // it lands (#243), which is not one.
      const answering = whyOf(context) === "refresh";
      if (answering) {
        for (const { run } of registered.ticks) await run();
      }
      /** What this render registers, replacing the last render's. */
      const collected = { ticks: [], view };
      const root = [];
      collecting = collected;
      selected = selectedOf(context);
      try {
        write({ type: component, props, key: undefined }, "", callbacks, cells, used, root);
      } finally {
        collecting = null;
        selected = null;
      }
      for (const path of cells.keys()) {
        if (!used.has(path)) cells.delete(path);
      }
      tables.set(number, callbacks);
      // Keep the last two renders' tables: an event raised on the tree the
      // user saw is delivered even if the view has rendered since.
      for (const render of tables.keys()) {
        if (render < number - 1) tables.delete(render);
      }
      if (root.length !== 1) {
        throw new Error("a view renders one root node");
      }
      registered = collected;
      // What Pane is asked to wait before the tree again: the soonest the
      // view's intervals ask for. Pending data waits for none of it: the
      // moment it lands asks for a drawing itself (#243).
      let refreshAfterMs = null;
      for (const { ms } of collected.ticks) {
        refreshAfterMs = Math.min(refreshAfterMs ?? Infinity, ms);
      }
      return {
        tree: JSON.stringify({ version: COMPONENT_SET, root: root[0] }),
        refreshAfterMs,
      };
    },
    async handleEvent(event) {
      // The pop event: the view above this one popped. Its payload is the
      // result that pop answered; the view re-renders after it, as after
      // every event.
      if (event.callback === POP_CALLBACK) {
        const run = onPop;
        onPop = null;
        if (run !== null) {
          await run(popResultOf(event.payload));
        }
        return NOTHING_NEXT;
      }
      // A press of a button the tree named: the table of the render the
      // user saw holds it. An older event is stale, dropped. The listener
      // is told the event's text — its payload's `value` (a field's, a
      // control's) or `key` (a key pressed) — which the listeners that
      // take none ignore.
      const run = tables.get(event.render)?.get(event.callback);
      let navigation = null;
      if (run !== undefined) {
        // The listener is told the event's payload as it is: a text
        // listener reads its text (its `value` or `key`), a value listener
        // what its `value` or `values` carries whatever type it is (#241),
        // a canvas's the event it names (#242).
        navigation = await run(event.payload);
      }
      // What the listener answered next: at most one of a push, a replace
      // and a pop is acted on — a pop first, then a replace, then a push,
      // as Pane does; the `onPop` a push was given stays here, never sent.
      const given = navigation === null || typeof navigation !== "object" ? null : navigation;
      const outcome = { push: null, replace: null, pop: null };
      if (given !== null) {
        if (given.push != null) outcome.push = viewOf(given.push);
        if (given.replace != null) outcome.replace = viewOf(given.replace);
        if (given.pop !== undefined) outcome.pop = given.pop ?? null;
        if (typeof given.onPop === "function") onPop = given.onPop;
      }
      return outcome;
    },
  };
}

/** What a value-carrying payload names: its `value` (a field's, a
 * control's), its `key` (a key pressed) or its `values` (a form's
 * submission, keyed by the fields' ids, #241) — a string, a boolean, a
 * list or an object; `undefined` when the payload names none.
 */
function payloadValue(payload) {
  try {
    const held = JSON.parse(payload);
    if (held && typeof held === "object") {
      if ("values" in held) return held.values;
      if ("value" in held) return held.value;
      if ("key" in held) return held.key;
    }
  } catch {
    // A payload Pane does not name a value in: nothing to tell.
  }
  return undefined;
}

/** The text a value-carrying payload names: its `value` (a field's, a
 * control's) or its `key` (a key pressed). An empty string when it names
 * none. */
function payloadText(payload) {
  try {
    const held = JSON.parse(payload);
    for (const field of ["value", "key"]) {
      if (typeof held?.[field] === "string") return held[field];
    }
  } catch {
    // A payload that is not JSON names no text.
  }
  return "";
}

/** The result the pop event's payload carries: `"…"` for a pop that
 * answered one, `null` for a pop that carried none (the back key's). */
function popResultOf(payload) {
  try {
    const result = JSON.parse(payload)?.pop;
    return typeof result === "string" ? result : null;
  } catch {
    return null;
  }
}

/** The view `target` names: a view `createView` made, or an element,
 * rendered as the tree of a new view of its own. */
function viewOf(target) {
  if (target !== null && typeof target === "object" && typeof target.render === "function") {
    return target;
  }
  if (target !== null && typeof target === "object" && typeof target.type === "function") {
    return createView(target.type, target.props);
  }
  throw new Error("a push or replace names what it opens: an element or a view");
}

/** What a listener returns to push a view above this one: `target`, an
 * element or a view, this view staying below it. `onPop`, when given, runs
 * with the result the pushed view's pop answered — or none, the back
 * key's — before this view re-renders. */
export function push(target, onPop) {
  return { push: viewOf(target), onPop };
}

/** What a listener returns to replace this view with `target`: this view
 * is dropped, the views below it staying. */
export function replace(target) {
  return { replace: viewOf(target) };
}

/** What a listener returns to pop this view, answering `result` (or an
 * empty one) to the view below. */
export function pop(result) {
  return { pop: result ?? "" };
}

/** A press that pushes a view — the Raycast-style `Action.Push`, as a
 * button's `onClick`: `Push(<Detail />)` where Raycast writes
 * `<Action.Push target={<Detail />} />`. `target` is an element or a view;
 * `onPop`, when given, runs with the result the pushed view's pop
 * answered (or none, the back key's) before this view re-renders. */
export function Push(target, onPop) {
  return () => push(target, onPop);
}

/**
 * Measures `text` in `style` (#242, `pane:extension/view.measure-text`):
 * the width and height it occupies when the canvas draws it, laid out
 * exactly, at the size and weight the tree's text styles resolve to. A
 * canvas's drawing calls this while the view renders, so the text it
 * draws fits what it says. `style` names what a canvas text operation
 * does: `{"style": "caption", "size": 13, "weight": 400}`.
 */
export function measureText(text, style = {}) {
  const held = {};
  if (style.style !== undefined) held.style = style.style;
  if (style.size !== undefined) held.size = style.size;
  if (style.weight !== undefined) held.weight = style.weight;
  return witMeasureText(text, JSON.stringify(held));
}

/** Why Pane asks for the tree, as `context` names: the render answering
 * the refresh the view asked for is where an interval's work runs; a
 * drawing the view's own work asked for (#243) is not one. `""` when it
 * says none. */
function whyOf(context) {
  try {
    return JSON.parse(context)?.why ?? "";
  } catch {
    return "";
  }
}

/** The selected item's key `context` names, or `null` when it says none. */
function selectedOf(context) {
  try {
    const named = JSON.parse(context)?.selected;
    return typeof named === "string" ? named : null;
  } catch {
    return null;
  }
}

/** The view `context` names — its id, what `ask-to-render` asks for — or 0
 * when it says none. */
function viewIdOf(context) {
  try {
    const named = JSON.parse(context)?.view;
    return typeof named === "number" && named > 0 ? named : 0;
  } catch {
    return 0;
  }
}

/** The render number `context` names, or `fallback` when it says none. */
function numberOf(context, fallback) {
  try {
    const named = JSON.parse(context)?.render;
    return typeof named === "number" && named > 0 ? named : fallback;
  } catch {
    return fallback;
  }
}
