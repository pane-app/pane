// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's no-view sample in JavaScript: commands that run without opening a
// screen (`"mode": "no-view"` in `pane.json`), and one view command, all
// served by one component. Each receives its launch record: how it was
// launched (by the user or in the background, and from where), the text
// sent through its alias or as a fallback, and the context another command
// passed. Commands, answers and errors match the Rust no-view sample
// (guests/sample-no-view) and the TypeScript one.
//
// - "Report launch" answers its launch record, and keeps it in its settings
//   for "Last launches". Sent "fail", it answers an error, which never
//   pauses the extension; sent "crash", it crashes, and three crashes within
//   five minutes pause the extension.
// - "Tick" runs every minute on its own schedule, in the background,
//   counting its runs and keeping its last launch record.
// - "Last launches" answers what "Report launch" and "Tick" kept.
// - "Launch" launches the command its text names, passing it the context
//   {"from":"launch"}: `report` (a command of this package), or
//   `<package identity>#report` (one of another package), user-initiated,
//   or in the background when the text starts with `background `.
// - "Show launch" is a view command: its list shows its launch record.
// @ts-check
import { launch } from "pane:extension/commands@0.1.0";
import { get, set } from "pane:extension/settings@0.1.0";

/** The settings key holding the last launch record "Report launch" ran with. */
const REPORT = "report";
/** The settings key holding how many times "Tick" ran. */
const TICKS = "ticks";
/** The settings key holding the last launch record "Tick" ran with. */
const TICK = "tick";
/** The context "Launch" passes the command it launches. */
const CONTEXT = '{"from":"launch"}';

/**
 * `record` in words: its type and source, then what it was given.
 * @param {import("@pane/extension").LaunchRecord} record
 * @returns {string}
 */
function describe(record) {
  const args = record.arguments.length === 0
    ? "none"
    : record.arguments.map(({ name, value }) => `${name}=${value}`).join(", ");
  return (
    `${record.launchType} from ${record.source}; fallback text: ${record.fallbackText ?? "none"}; ` +
    `context: ${record.context ?? "none"}; arguments: ${args}`
  );
}

/**
 * What "Report launch" answers for `record`.
 * @param {import("@pane/extension").LaunchRecord} record
 * @returns {string}
 */
function report(record) {
  if (record.fallbackText === "fail") throw new Error("Report launch fails on request");
  if (record.fallbackText === "crash") {
    // Resolving with something other than a string is a crash, unlike
    // throwing, which is an error the extension answers with.
    return /** @type {string} */ (/** @type {unknown} */ (undefined));
  }
  const described = describe(record);
  set(REPORT, described);
  return `Report: ${described}`;
}

/**
 * What "Tick" answers for `record`, counting the run.
 * @param {import("@pane/extension").LaunchRecord} record
 * @returns {string}
 */
function tick(record) {
  const saved = get(TICKS);
  const counted = saved === null ? 0 : Number.parseInt(saved, 10);
  const ticks = (Number.isNaN(counted) ? 0 : counted) + 1;
  set(TICKS, String(ticks));
  set(TICK, describe(record));
  return `Ticked ${ticks} times`;
}

/**
 * What "Last launches" answers: what "Report launch" and "Tick" kept.
 * @returns {string}
 */
function last() {
  return (
    `Last report: ${get(REPORT) ?? "none"}. Ticks: ${get(TICKS) ?? "0"}; ` +
    `last tick: ${get(TICK) ?? "none"}`
  );
}

/**
 * Launches the command `text` names, as "Launch" does.
 * @param {string | null | undefined} text
 * @returns {string}
 */
function launchNamed(text) {
  if (text == null) {
    throw new Error(
      "Launch needs the command to launch: send it `report`, `background report` or " +
        "`<package identity>#report`",
    );
  }
  const background = text.startsWith("background ");
  const named = background ? text.slice("background ".length).trim() : text;
  const at = named.lastIndexOf("#");
  const target = at < 0
    ? { source: null, command: named }
    : { source: named.slice(0, at), command: named.slice(at + 1) };
  try {
    launch(target, background ? "background" : "user-initiated", [], CONTEXT);
  } catch (error) {
    const payload = /** @type {{ payload?: unknown }} */ (error).payload;
    throw new Error(typeof payload === "string" ? payload : String(error));
  }
  return background ? `Launched ${named} in the background` : `Launched ${named}`;
}

/** @type {import("@pane/extension").Command} */
export const command = {
  async render(record) {
    return {
      title: "Launch record",
      items: [
        { id: "type", title: `Launch type: ${record.launchType}` },
        { id: "source", title: `Source: ${record.source}` },
        { id: "fallback-text", title: `Fallback text: ${record.fallbackText ?? "none"}` },
        { id: "context", title: `Context: ${record.context ?? "none"}` },
      ],
    };
  },
  async run(id, record) {
    switch (id) {
      case "report":
        return report(record);
      case "tick":
        return tick(record);
      case "last":
        return last();
      case "launch":
        return launchNamed(record.fallbackText);
      default:
        throw new Error(`unknown command: ${id}`);
    }
  },
};
