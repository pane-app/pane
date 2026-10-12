// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's sample command in TypeScript: a list with one action per item, a
// form the "Greet someone" command of the same package answers on the
// designed tree, a color picker the command draws itself as a canvas of its
// designed view, and a root result computed from the query ("reverse
// <text>"). Items, titles, results, errors and drawings match the Rust
// sample (guests/sample-rust) and the JavaScript sample.
import type { CanvasEvent, Draw, Paint } from "@pane-app/extension/view";
import type {
  Command,
  DesignedView,
  LaunchRecord,
  List,
  RootResult,
  RootResults,
} from "@pane-app/extension";
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
import type { Element, FormSubmittedValues } from "@pane-app/extension/view";
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
 * The color picker's colors, [hue, light, medium, dark]: a column of swatches
 * per hue, one row per shade.
 */
const COLORS: [string, number, number, number][] = [
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

/** "#RRGGBB" for 0xRRGGBB. */
const hex = (rgb: number) => `#${rgb.toString(16).padStart(6, "0").toUpperCase()}`;
const clamp = (n: number, max: number) => Math.min(Math.max(n, 0), max);

/** How each arrow key moves the choice, as (columns, rows). */
const MOVES: Record<"left" | "right" | "up" | "down", [number, number]> = {
  left: [-1, 0],
  right: [1, 0],
  up: [0, -1],
  down: [0, 1],
};

/** The frame colour the chosen swatch and the hex code are drawn in. */
const FRAME = "#f1f3f5";

/** A swatch's own colour, drawn exactly as the sample authored it. */
const swatch = (fill: number): Paint => ({ raw: hex(fill) });

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
  const ops: Draw[] = [
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
  const key = (name: string) => {
    if (name === "home") setChoice({ column: 0, row });
    else if (name === "end") setChoice({ column: COLUMNS - 1, row });
    else if (Object.hasOwn(MOVES, name)) {
      const [dx, dy] = MOVES[name as "left" | "right" | "up" | "down"];
      setChoice({
        column: clamp(column + dx, COLUMNS - 1),
        row: clamp(row + dy, ROWS - 1),
      });
    }
  };
  // Only a press on the grid chooses a swatch and starts a drag; a move
  // during one chooses under the pointer.
  const pointed = (event: CanvasEvent, press: boolean) => {
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
        onPointerDown: (event: CanvasEvent) => pointed(event, true),
        onPointerMove: (event: CanvasEvent) => pointed(event, false),
        onPointerUp: () => {
          dragging.current = false;
        },
        ops,
      }),
    ],
  });
}

/** Chooses the swatch nearest to `x`, `y`. */
function at(x: number, y: number) {
  return {
    column: clamp(Math.floor(x / STEP), COLUMNS - 1),
    row: clamp(Math.floor(y / STEP), ROWS - 1),
  };
}

/**
 * Runs the action of the item `itemId`, showing a toast with what it did;
 * each item's action is this with its id.
 */
async function act(itemId: string): Promise<void> {
  showToast({ title: await outcome(itemId) });
}

/** Does what the item `itemId`'s action does; the text its toast shows. */
async function outcome(itemId: string): Promise<string> {
  switch (itemId) {
    case "greet":
      return "Hello from the TypeScript guest";
    case "wait":
      // A native component-model async import; the command suspends here.
      await waitFor(50_000_000);
      return "Waited 50 ms inside the TypeScript guest";
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
      return "Ran the Windows-only action in the TypeScript guest";
    case "not-windows":
      return "Ran the macOS and Linux action in the TypeScript guest";
    default:
      throw new Error(`unknown item: ${itemId}`);
  }
}

async function render(): Promise<List> {
  return {
    title: "TypeScript sample",
    items: [
      { id: "greet", title: "Say hello", subtitle: "Answer from the TypeScript guest", onAction: () => act("greet") },
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
        onAction: async () => launch({ command: "color" }, "user-initiated", [], null),
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
}

async function openView(
  commandId: string,
  _launch: LaunchRecord,
): Promise<DesignedView> {
  if (commandId === "form") {
    return createView(GreetingForm);
  }
  if (commandId !== "color") {
    throw new Error(`unknown designed view: ${commandId}`);
  }
  return createView(ColorPicker);
}

export const command: Command = { render, openView };

/**
 * "reverse <text>" typed into root search lists the text reversed, which
 * Enter copies, and "pane website" lists Pane's website, which Enter opens;
 * other queries have no results.
 */
async function resultsFor(query: string): Promise<RootResult[]> {
  if (query === "pane website") {
    return [
      {
        id: "website",
        title: "Pane's website",
        subtitle: "Opened by the TypeScript guest",
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
      subtitle: "Reversed by the TypeScript guest",
      action: { tag: "copy", val: reversed },
    },
  ];
}

export const rootResults: RootResults = { resultsFor };

/**
 * The "Greet someone" command's view (#241): the greeting form — every
 * field the designed tree's forms offer, so the sample shows each kind.
 * A submission validates the name and answers with the greeting; the
 * fields' values arrive with the submission, keyed by their `id`s, and
 * the name's is remembered: Pane keeps it as the package's settings and
 * prefills it the next time the form opens.
 */
function GreetingForm(): Element {
  const [answer, setAnswer] = useState("");
  const [error, setError] = useState<string | null>(null);
  const submit = (values: FormSubmittedValues) => {
    const submitted = z.safeParse(Greeting, values);
    if (!submitted.success) {
      // The first problem marks its field in the tree the view answers
      // with, as its validation error.
      setError(submitted.error.issues[0].message);
      return;
    }
    setError(null);
    const { name, greeting } = submitted.data;
    setAnswer(`${GREETINGS[greeting]}, ${name}, from the TypeScript guest`);
  };
  // The answer draws over the form as its first child, the form staying
  // the tree's root: the window keeps a field's state by the path of its
  // key, and wrapping the form in a column would move every field's path
  // — losing the text, the caret and the focus the user left in them.
  const form = (
    <Form key="form" navigationTitle="Greet someone" submitTitle="Greet" onSubmit={submit}>
      {answer !== "" && <Text style="title">{answer}</Text>}
      <Form.TextField
        key="name"
        id="name"
        title="Name"
        placeholder="Ada Lovelace"
        remember
        autoFocus
        error={error ?? undefined}
      />
      <Form.PasswordField key="secret" id="secret" title="Secret" />
      <Form.TextArea key="notes" id="notes" title="Notes" />
      <Form.DatePicker key="day" id="day" title="Day" />
      <Form.DateTimePicker key="at" id="at" title="At" />
      <Form.Dropdown
        key="greeting"
        id="greeting"
        title="Greeting"
        options={[
          { value: "hello", label: "Hello", section: "Plain" },
          { value: "morning", label: "Good morning", section: "Warm" },
          { value: "welcome", label: "Welcome", section: "Warm" },
        ]}
        defaultValue="hello"
      />
      <Form.TagPicker
        key="tags"
        id="tags"
        title="Tags"
        options={[
          { value: "friend", label: "Friend" },
          { value: "colleague", label: "Colleague" },
          { value: "family", label: "Family" },
        ]}
      />
      <Form.FilePicker key="file" id="file" title="File" />
      <Form.FolderPicker key="folder" id="folder" title="Folder" allowMultiple />
      <Form.Checkbox key="updates" id="updates" label="Send updates" />
      <Form.Toggle key="quiet" id="quiet" label="Quiet mode" />
    </Form>
  );
  return form;
}
