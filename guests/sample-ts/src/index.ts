// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's sample command in TypeScript: a list with one action per item, a
// form, a color picker the command draws itself and a root result computed
// from the query ("reverse <text>"). Items, titles, results,
// errors and drawings match the Rust sample (guests/sample-rust) and the
// JavaScript sample.
import type {
  Command,
  CustomView,
  FieldValue,
  Form,
  FormError,
  Frame,
  Key,
  List,
  RootResult,
  RootResults,
  Shape,
  ViewEvent,
} from "@pane/extension";
import { showToast } from "@pane/extension/feedback";
import { waitFor }from "wasi:clocks/monotonic-clock@0.3.0";
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

/** The "form" item's form: a name to greet and a greeting to choose. */
const GREETING_FORM: Form = {
  title: "Greet someone",
  fields: [
    { id: "name", label: "Name", kind: { tag: "text", val: { placeholder: "Ada Lovelace" } } },
    {
      id: "greeting",
      label: "Greeting",
      kind: {
        tag: "choice",
        val: Object.entries(GREETINGS).map(([id, label]) => ({ id, label })),
      },
    },
  ],
  submitLabel: "Greet",
};

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
const MOVES: Record<Exclude<Key, "home" | "end">, [number, number]> = {
  left: [-1, 0],
  right: [1, 0],
  up: [0, -1],
  down: [0, 1],
};

/**
 * An open color picker: a grid of swatches and a preview of the chosen color.
 * Arrow keys, Home and End move the choice; pressing or dragging the pointer
 * over the grid chooses the swatch under it. Pane creates one per opened view
 * (`openView`) and drops it when the view closes.
 */
class ColorPicker implements CustomView {
  // Blue.
  column = 5;
  row = 1;
  dragging = false;

  async render(): Promise<Frame> {
    const [hue, ...shades] = COLORS[this.column];
    const chosen = shades[this.row];
    const shade = SHADES[this.row];
    const shapes: Shape[] = [
      // A light frame around the chosen swatch.
      { tag: "rect", val: { x: this.column * STEP, y: this.row * STEP, width: STEP, height: STEP, fill: 0xf1f3f5 } },
    ];
    COLORS.forEach(([, ...column], x) =>
      column.forEach((fill, y) =>
        shapes.push({
          tag: "rect",
          val: { x: x * STEP + 2, y: y * STEP + 2, width: SWATCH, height: SWATCH, fill },
        }),
      ),
    );
    shapes.push(
      { tag: "rect", val: { x: COLUMNS * STEP + 12, y: 2, width: 64, height: 64, fill: chosen } },
      { tag: "text", val: { x: COLUMNS * STEP + 12, y: 74, content: hex(chosen), color: 0xf1f3f5 } },
    );
    return {
      width: COLUMNS * STEP + 88,
      height: ROWS * STEP,
      shapes,
      value: `${shade ? `${shade} ${hue.toLowerCase()}` : hue}, ${hex(chosen)}`,
    };
  }

  async handleEvent(event: ViewEvent): Promise<void> {
    switch (event.tag) {
      case "key":
        if (event.val === "home") this.column = 0;
        else if (event.val === "end") this.column = COLUMNS - 1;
        else {
          const [dx, dy] = MOVES[event.val];
          this.column = clamp(this.column + dx, COLUMNS - 1);
          this.row = clamp(this.row + dy, ROWS - 1);
        }
        break;
      case "pointer-down": {
        const { x, y } = event.val;
        // Only a press on the grid chooses a swatch and starts a drag.
        if (x >= 0 && x < COLUMNS * STEP && y >= 0 && y < ROWS * STEP) {
          this.dragging = true;
          this.choose(x, y);
        }
        break;
      }
      case "pointer-move":
        if (this.dragging) this.choose(event.val.x, event.val.y);
        break;
      case "pointer-up":
        this.dragging = false;
        break;
    }
  }

  /** Chooses the swatch nearest to `x`, `y`. */
  choose(x: number, y: number) {
    this.column = clamp(Math.floor(x / STEP), COLUMNS - 1);
    this.row = clamp(Math.floor(y / STEP), ROWS - 1);
  }
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
      { id: "form", title: "Greet someone", subtitle: "Fill in a form the guest checks", form: GREETING_FORM },
      {
        id: "color",
        title: "Choose a color",
        subtitle: "Pick a color in a view the guest draws",
        customView: { title: "Choose a color", label: "Color", role: "color-well" },
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

async function submitForm(itemId: string, values: FieldValue[]): Promise<string> {
  if (itemId !== "form") {
    throw { message: `unknown form: ${itemId}` } satisfies FormError;
  }
  const submitted = Object.fromEntries(values.map(({ id, value }) => [id, value]));
  const parsed = z.safeParse(Greeting, submitted);
  if (!parsed.success) {
    // A thrown FormError is shown next to its field.
    const issue = parsed.error.issues[0];
    throw { field: String(issue.path[0]), message: issue.message } satisfies FormError;
  }
  const { name, greeting } = parsed.data;
  return `${GREETINGS[greeting]}, ${name}, from the TypeScript guest`;
}

async function openView(itemId: string): Promise<CustomView> {
  if (itemId !== "color") {
    throw new Error(`unknown view: ${itemId}`);
  }
  return new ColorPicker();
}

export const command: Command = { render, submitForm, openView };

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
