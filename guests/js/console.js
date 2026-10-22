// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// `console` for JS/TS commands, which QuickJS lacks. What a command logs
// goes to its package's extension log, which its author sees while
// developing it ("Logs for <title>" in Pane, and the development session's
// log file); for a package not being developed, Pane keeps only its most
// recent lines in memory.
//
// `console.debug`, `log`, `info`, `warn` and `error` write a line with its
// level (debug and info on standard output, warnings and errors on
// standard error), in the level prefixes Pane reads (`<7>`, `<6>`, `<4>`,
// `<3>`), on every line of a message that spans several. Arguments are
// formatted as Node formats them, roughly: text as it is, an `Error` as its
// stack, other values as JSON where they can be, `%s`, `%d`, `%i`, `%f`,
// `%o`, `%O`, `%j` and `%%` substituted in a first argument that is text.
// `console.trace` writes its message with the stack at debug level, and
// `console.assert` an error when its condition is false.
//
// It writes through `__paneWrite`, a native function Pane's componentizer
// defines (tools/componentize-js/patches/0005-native-output.patch), and
// removes that global. The adapter (adapt.js) imports this module, so every
// JS/TS component has `console`.

const write = globalThis.__paneWrite;
delete globalThis.__paneWrite;

/** Where each level goes, and its prefix. */
const LEVELS = {
  debug: [1, "<7>"],
  info: [1, "<6>"],
  warn: [2, "<4>"],
  error: [2, "<3>"],
};

/** `value` as a log line shows it. */
function inspect(value) {
  if (typeof value === "string") return value;
  if (value instanceof Error) return value.stack ? `${value}\n${value.stack}`.trimEnd() : String(value);
  if (value === undefined) return "undefined";
  if (typeof value === "function") return `[Function ${value.name || "(anonymous)"}]`;
  if (typeof value === "bigint") return `${value}n`;
  if (typeof value === "symbol") return value.toString();
  if (value !== null && typeof value === "object") {
    try {
      return JSON.stringify(value, (_key, inner) => (typeof inner === "bigint" ? `${inner}n` : inner));
    } catch {
      return String(value);
    }
  }
  return String(value);
}

/** The text of a call's arguments. */
function format(args) {
  if (args.length === 0) return "";
  let rest = args;
  let first = "";
  if (typeof args[0] === "string" && args[0].includes("%")) {
    let next = 1;
    first = args[0].replace(/%([sdifoOj%])/g, (whole, kind) => {
      if (kind === "%") return "%";
      if (next >= args.length) return whole;
      const value = args[next++];
      switch (kind) {
        case "s":
          return typeof value === "string" ? value : inspect(value);
        case "d":
        case "i":
          return typeof value === "bigint" ? `${value}n` : String(kind === "i" ? Math.trunc(Number(value)) : Number(value));
        case "f":
          return String(Number(value));
        default:
          return inspect(value);
      }
    });
    rest = args.slice(next);
  } else {
    first = inspect(args[0]);
    rest = args.slice(1);
  }
  return [first, ...rest.map(inspect)].join(" ");
}

/** Writes `text` at `level`, each of its lines with the level's prefix. */
function log(level, text) {
  if (typeof write !== "function") return;
  const [stream, prefix] = LEVELS[level];
  const lines = text.replace(/\n+$/, "").split("\n").map((line) => prefix + line);
  write(stream, `${lines.join("\n")}\n`);
}

/** Logs what a handler threw, with its stack: the adapter calls this. */
export function logThrown(thrown) {
  log("error", inspect(thrown));
}

const console = {
  debug: (...args) => log("debug", format(args)),
  log: (...args) => log("info", format(args)),
  info: (...args) => log("info", format(args)),
  warn: (...args) => log("warn", format(args)),
  error: (...args) => log("error", format(args)),
  trace: (...args) => {
    const stack = (new Error().stack ?? "").split("\n").slice(1).join("\n");
    log("debug", `Trace: ${format(args)}${stack ? `\n${stack}` : ""}`);
  },
  assert: (condition, ...args) => {
    if (!condition) log("error", `Assertion failed${args.length > 0 ? `: ${format(args)}` : ""}`);
  },
};

if (typeof globalThis.console !== "object" || globalThis.console === null) {
  globalThis.console = console;
}
