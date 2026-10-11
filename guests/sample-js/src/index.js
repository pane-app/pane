// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's sample command in JavaScript: a list with one action per item, a
// a form the "Greet someone" command of the same package answers on the
// designed tree, a color picker the command draws itself and a root result
// computed from the query ("reverse <text>"). Items, titles, results and
// errors match the Rust sample (guests/sample-rust) and the TypeScript
// sample. The JSDoc types let TypeScript check this file against Pane's
// contract; they are optional.
// @ts-check
/** @typedef {import("@pane-app/extension/view").CanvasEvent} CanvasEvent */
import { launch } from "pane:extension/commands@0.1.0";
import { jsxs } from "@pane-app/extension/jsx-runtime";
import {
  Canvas,
  Column,
  Form,
  Text,
  createView,
  measureText,
  useRef,
  useState,
} from "@pane-app/extension/view";
import { showToast } from "@pane-app/extension/feedback";
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";
import * as z from "zod/mini";

// Module top-level code runs once, when the component is built, and its state
// is part of the snapshot every instance starts from. Pure setup such as these
// schemas belongs here; secrets, IDs, timestamps and random values do not.
const PORT_RANGE = "port must be between 1 and 65535";
const Settings = z.object({
  name: z.string().check(z.minLength(1, "name must not be empty")),
  port: z.int().check(z.minimum(1, PORT_RANGE), z.maximum(65535, PORT_RANGE)),
});

/** The greeting form's options, by id. */
const GREETINGS = { hello: "Hello", morning: "Good morning", welcome: "Welcome" };

// Checks the submitted values in field order; the first problem is reported.
// Lengths count characters (code points), like the Rust sample.
const Greeting = z.object({
  name: z
    .string()
    .check(
      z.trim(),
      z.minLength(1, "Enter a name"),
      z.refine((name) => [...name].length <= 40, "Use at most 40 characters"),
    ),
  greeting: z.enum(["hello", "morning", "welcome"], "Choose a greeting"),
});

/**
 * The color picker's colors, [hue, light, medium, dark]: a column of
 * swatches per hue, one row per shade.
 * @type {[string, number, number, number][]}
 */
const COLORS = [
  ["Red", 0xef9a9a, 0xe53935, 0xb71c1c],
  ["Orange", 0xffcc80, 0xfb8c00, 0xe65100],
  ["Yellow", 0xfff59d, 0xfdd835, 0xf57f17],
  ["Green", 0xa5d6a7, 0x43a047, 0x1b5e20],
  ["Teal", 0x80cbc4, 0x00897b, 0x004d40],
  ["Blue", 0x90caf9, 0x1e88e5, 0x0d47a1],
  ["Purple", 0xce93d8, 0x8e24aa, 0x4a148c],
  ["Pink", 0xf48fb1, 0xd81b60, 0x880e4f],
];
const SHADES = ["Light", "", "Dark"];
/** A swatch's size, and the distance from one swatch to the next. */
const SWATCH = 32;
const STEP = 36;
const COLUMNS = COLORS.length;
const ROWS = SHADES.length;

/** The frame colour the chosen swatch and the hex code are drawn in. */
const FRAME = "#f1f3f5";

/** "#RRGGBB" for 0xRRGGBB. */
const hex = (/** @type {number} */ rgb) => `#${rgb.toString(16).padStart(6, "0").toUpperCase()}`;

/** A swatch's own colour, drawn exactly as the sample authored it. */
const swatch = (/** @type {number} */ fill) => ({ raw: hex(fill) });

/** How each arrow key moves the choice, as (columns, rows). */
const MOVES = { left: [-1, 0], right: [1, 0], up: [0, -1], down: [0, 1] };

/** Moves the choice by `columns` and `rows`, clamped at the edges. */
const moved = (/** @type {number} */ column, /** @type {number} */ row,
               /** @type {number} */ columns, /** @type {number} */ rows) => ({
  column: clamp(column + columns, COLUMNS - 1),
  row: clamp(row + rows, ROWS - 1),
});

const clamp = (/** @type {number} */ n, /** @type {number} */ max) => Math.min(Math.max(n, 0), max);

/** Chooses the swatch nearest to `x`, `y`. */
const at = (/** @type {number} */ x, /** @type {number} */ y) => ({
  column: clamp(Math.floor(x / STEP), COLUMNS - 1),
  row: clamp(Math.floor(y / STEP), ROWS - 1),
});

/**
 * An open color picker: a grid of swatches and a preview of the chosen
 * color, drawn as the canvas of its designed view. Arrow keys, Home and
 * End move the choice; pressing or dragging the pointer over the grid
 * chooses the swatch under it. Pane opens one per opened view (the
 * "color" command's `openView`) and drops it when the view closes.
 */
function ColorPicker() {
  // Blue.
  const [choice, setChoice] = useState({ column: 5, row: 1 });
  const dragging = useRef(false);
  const { column, row } = choice;
  const [hue, ...shades] = COLORS[column];
  const chosen = shades[row];
  const shade = SHADES[row];
  const code = hex(chosen);
  // A light frame around the chosen swatch, then the swatches, the
  // preview and its hex code. The hex code is measured
  // (`pane:extension/view.measure-text`), so the text drawn under the
  // preview fits what it says.
  const measured = measureText(code, { style: "caption" });
  /** @type {import("@pane-app/extension/view").Draw[]} */
  const ops = [
    { op: "rect", x: column * STEP, y: row * STEP, width: STEP, height: STEP, fill: FRAME },
  ];
  for (let x = 0; x < COLUMNS; x += 1) {
    const [, ...shades] = COLORS[x];
    for (let y = 0; y < ROWS; y += 1) {
      ops.push({
        op: "rect",
        x: x * STEP + 2,
        y: y * STEP + 2,
        width: SWATCH,
        height: SWATCH,
        fill: swatch(shades[y]),
      });
    }
  }
  ops.push(
    { op: "rect", x: COLUMNS * STEP + 12, y: 2, width: 64, height: 64, fill: swatch(chosen) },
    {
      op: "text",
      x: COLUMNS * STEP + 12,
      y: 68 + measured.height,
      text: code,
      style: "caption",
      color: FRAME,
    },
  );
  const key = (/** @type {string} */ name) => {
    if (name === "home") setChoice({ column: 0, row });
    else if (name === "end") setChoice({ column: COLUMNS - 1, row });
    else if (Object.hasOwn(MOVES, name)) {
      const [columns, rows] =
        /** @type {[number, number]} */ (MOVES[/** @type {"left" | "right" | "up" | "down"} */ (name)]);
      setChoice(moved(column, row, columns, rows));
    }
  };
  // Only a press on the grid chooses a swatch and starts a drag; a move
  // during one chooses under the pointer.
  const pointed = (
    /** @type {import("@pane-app/extension/view").CanvasEvent} */ event,
    /** @type {boolean} */ press,
  ) => {
    if (event.event !== "pointer-down" && event.event !== "pointer-move") return;
    const { x, y } = event;
    const onGrid = x >= 0 && x < COLUMNS * STEP && y >= 0 && y < ROWS * STEP;
    if (press) {
      if (onGrid) {
        dragging.current = true;
        setChoice(at(x, y));
      }
    } else if (dragging.current) {
      setChoice(at(x, y));
    }
  };
  return jsxs(Column, {
    key: "picker",
    navigationTitle: "Choose a color",
    children: [
      jsxs(Canvas, {
        key: "grid",
        width: COLUMNS * STEP + 88,
        height: ROWS * STEP,
        role: "color-well",
        label: "Color",
        value: `${shade ? `${shade} ${hue.toLowerCase()}` : hue}, ${code}`,
        onKey: key,
        onPointerDown: (/** @type {CanvasEvent} */ event) => pointed(event, true),
        onPointerMove: (/** @type {CanvasEvent} */ event) => pointed(event, false),
        onPointerUp: () => {
          dragging.current = false;
        },
        ops,
      }),
    ],
  });
}

/**
 * Runs the action of the item `itemId`, showing a toast with what it did;
 * each item's action is this with its id.
 * @param {string} itemId
 * @returns {Promise<void>}
 */
async function act(itemId) {
  showToast({ title: await outcome(itemId) });
}

/**
 * Does what the item `itemId`'s action does, and resolves with the text
 * its toast shows.
 * @param {string} itemId
 * @returns {Promise<string>}
 */
async function outcome(itemId) {
  switch (itemId) {
    case "greet":
      return "Hello from the JavaScript guest";
    case "wait":
      // A native component-model async import; the command suspends here.
      await waitFor(50_000_000);
      return "Waited 50 ms inside the JavaScript guest";
    case "validate": {
      const parsed = z.safeParse(Settings, { name: "Pane", port: 70000 });
      if (!parsed.success) {
        // The thrown error's message is shown to the user as the command's error.
        throw new Error(`Invalid settings: ${parsed.error.issues[0].message}`);
      }
      return `Settings are valid: ${parsed.data.name} on port ${parsed.data.port}`;
    }
    case "random":
      return String(Math.random());
    case "windows-only":
      return "Ran the Windows-only action in the JavaScript guest";
    case "not-windows":
      return "Ran the macOS and Linux action in the JavaScript guest";
    default:
      throw new Error(`unknown item: ${itemId}`);
  }
}

/** @type {import("@pane-app/extension").Command} */
export const command = {
  async render() {
    return {
      title: "JavaScript sample",
      items: [
        { id: "greet", title: "Say hello", subtitle: "Answer from the JavaScript guest", onAction: () => act("greet") },
        { id: "wait", title: "Wait briefly", subtitle: "Await a WASI 0.3 clock, then answer", onAction: () => act("wait") },
        { id: "validate", title: "Validate settings", subtitle: "Reject settings with an out-of-range port", onAction: () => act("validate") },
        { id: "random", title: "Roll a number", subtitle: "A random number from this instance", onAction: () => act("random") },
        {
          id: "form",
          title: "Greet someone",
          subtitle: "A form the guest checks, on the designed tree",
          onAction: async () =>
            launch({ command: "form" }, "user-initiated", [], null),
        },
        {
          id: "color",
          title: "Choose a color",
          subtitle: "Pick a color in a view the guest draws",
          onAction: async () =>
            launch({ command: "color" }, "user-initiated", [], null),
        },
        // Elsewhere Pane lists these as unavailable, says why, and never runs
        // their actions.
        {
          id: "windows-only",
          title: "Windows-only action",
          subtitle: "Declared to work on Windows only",
          platforms: ["windows"],
          onAction: () => act("windows-only"),
        },
        {
          id: "not-windows",
          title: "macOS and Linux action",
          subtitle: "Declared to work on macOS and Linux only",
          platforms: ["macos", "linux"],
          onAction: () => act("not-windows"),
        },
      ],
    };
  },

  async openView(/** @type {string} */ commandId) {
    if (commandId === "form") {
      return createView(GreetingForm);
    }
    if (commandId !== "color") {
      throw new Error(`unknown designed view: ${commandId}`);
    }
    return createView(ColorPicker);
  },
};

/** @type {import("@pane-app/extension").RootResults} */
export const rootResults = {
  // "reverse <text>" typed into root search lists the text reversed, which
  // Enter copies, and "pane website" lists Pane's website, which Enter
  // opens; other queries have no results.
  async resultsFor(query) {
    if (query === "pane website") {
      return [
        {
          id: "website",
          title: "Pane's website",
          subtitle: "Opened by the JavaScript guest",
          action: { tag: "open-url", val: "https://github.com/hoangvu12/pane" },
        },
      ];
    }
    const text = query.startsWith("reverse ") ? query.slice("reverse ".length).trim() : "";
    if (!text) {
      return [];
    }
    const reversed = [...text].reverse().join("");
    return [
      {
        id: "reversed",
        title: reversed,
        subtitle: "Reversed by the JavaScript guest",
        action: { tag: "copy", val: reversed },
      },
    ];
  },
};

/**
 * The "Greet someone" command's view (#241): the greeting form — every
 * field the designed tree's forms offer, so the sample shows each kind.
 * A submission validates the name and answers with the greeting; the
 * fields' values arrive with the submission, keyed by their `id`s, and
 * the name's is remembered: Pane keeps it as the package's settings and
 * prefills it the next time the form opens.
 * @returns {import("@pane-app/extension/view").Element}
 */
function GreetingForm() {
  const [answer, setAnswer] = useState("");
  const [error, setError] = useState(/** @type {string | null} */ (null));
  const submit = (/** @type {import("@pane-app/extension/view").FormSubmittedValues} */ values) => {
    const submitted = z.safeParse(Greeting, values);
    if (!submitted.success) {
      // The first problem marks its field in the tree the view answers
      // with, as its validation error.
      const issue = submitted.error.issues[0];
      setError(String(issue.message));
      return;
    }
    setError(null);
    const { name, greeting } = submitted.data;
    setAnswer(`${GREETINGS[greeting]}, ${name}, from the JavaScript guest`);
  };
  const form = jsxs(Form, {
    key: "form",
    navigationTitle: "Greet someone",
    submitTitle: "Greet",
    onSubmit: submit,
    // The answer draws over the form as its first child, the form staying
    // the tree's root: the window keeps a field's state by the path of
    // its key, and wrapping the form in a column would move every
    // field's path — losing the text, the caret and the focus the user
    // left in them.
    children: [
      ...(answer === "" ? [] : [jsxs(Text, { style: "title", children: [answer] })]),
      jsxs(Form.TextField, {
        id: "name",
        key: "name",
        title: "Name",
        placeholder: "Ada Lovelace",
        remember: true,
        autoFocus: true,
        error: error ?? undefined,
      }),
      jsxs(Form.PasswordField, { id: "secret", key: "secret", title: "Secret" }),
      jsxs(Form.TextArea, { id: "notes", key: "notes", title: "Notes" }),
      jsxs(Form.DatePicker, { id: "day", key: "day", title: "Day" }),
      jsxs(Form.DateTimePicker, { id: "at", key: "at", title: "At" }),
      jsxs(Form.Dropdown, {
        id: "greeting",
        key: "greeting",
        title: "Greeting",
        options: [
          { value: "hello", label: "Hello", section: "Plain" },
          { value: "morning", label: "Good morning", section: "Warm" },
          { value: "welcome", label: "Welcome", section: "Warm" },
        ],
        defaultValue: "hello",
      }),
      jsxs(Form.TagPicker, {
        id: "tags",
        key: "tags",
        title: "Tags",
        options: [
          { value: "friend", label: "Friend" },
          { value: "colleague", label: "Colleague" },
          { value: "family", label: "Family" },
        ],
      }),
      jsxs(Form.FilePicker, { id: "file", key: "file", title: "File" }),
      jsxs(Form.FolderPicker, { id: "folder", key: "folder", title: "Folder", allowMultiple: true }),
      jsxs(Form.Checkbox, { id: "updates", key: "updates", label: "Send updates" }),
      jsxs(Form.Toggle, { id: "quiet", key: "quiet", label: "Quiet mode" }),
    ],
  });
  return form;
}
