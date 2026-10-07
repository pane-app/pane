// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's no-view sample in TypeScript: commands that run without opening a
// screen (`"mode": "no-view"` in `pane.json`), and one view command, all
// served by one component. Each receives its launch record: how it was
// launched (by the user or in the background, and from where), the text
// sent through its alias or as a fallback, and the context another command
// passed. Commands, toasts and errors match the Rust no-view sample
// (guests/sample-no-view) and the JavaScript one.
//
// - "Report launch" shows its launch record in a toast, and keeps it in its
//   settings for "Last launches". Sent "fail", it answers an error, which
//   Pane shows as a failure toast and which never pauses the extension;
//   sent "crash", it crashes, and three crashes within five minutes pause
//   the extension.
// - "Tick" runs every minute on its own schedule, in the background,
//   counting its runs and keeping its last launch record.
// - "Last launches" shows what "Report launch" and "Tick" kept.
// - "Launch" launches the command its text names, passing it the context
//   {"from":"launch"}: `report` (a command of this package), or
//   `<package identity>#report` (one of another package), user-initiated,
//   or in the background when the text starts with `background `, and
//   shows a toast saying so.
// - "Show launch" is a view command: its list shows its launch record.
//
// A command launched in the background (a schedule's run, or one another
// command launched so) does its work but shows no toast.
import { launch } from "pane:extension/commands@0.1.0";
import { get, set } from "pane:extension/settings@0.1.0";
import type { Command, CommandRef, LaunchRecord, List } from "@pane/extension";
import { showToast } from "@pane/extension/feedback";

/** The settings key holding the last launch record "Report launch" ran with. */
const REPORT = "report";
/** The settings key holding how many times "Tick" ran. */
const TICKS = "ticks";
/** The settings key holding the last launch record "Tick" ran with. */
const TICK = "tick";
/** The context "Launch" passes the command it launches. */
const CONTEXT = '{"from":"launch"}';

/** `record` in words: its type and source, then what it was given. */
function describe(record: LaunchRecord): string {
  const args =
    record.arguments.length === 0
      ? "none"
      : record.arguments.map(({ name, value }) => `${name}=${value}`).join(", ");
  return (
    `${record.launchType} from ${record.source}; fallback text: ${record.fallbackText ?? "none"}; ` +
    `context: ${record.context ?? "none"}; arguments: ${args}`
  );
}

/**
 * What "Report launch" shows for `record` (sent "crash", it crashes
 * instead: see `run`).
 */
function report(record: LaunchRecord): string {
  if (record.fallbackText === "fail") throw new Error("Report launch fails on request");
  const described = describe(record);
  set(REPORT, described);
  return `Report: ${described}`;
}

/** What "Tick" would show for `record`, counting the run. */
function tick(record: LaunchRecord): string {
  const saved = get(TICKS);
  const counted = saved === null ? 0 : Number.parseInt(saved, 10);
  const ticks = (Number.isNaN(counted) ? 0 : counted) + 1;
  set(TICKS, String(ticks));
  set(TICK, describe(record));
  return `Ticked ${ticks} times`;
}

/** What "Last launches" shows: what "Report launch" and "Tick" kept. */
function last(): string {
  return (
    `Last report: ${get(REPORT) ?? "none"}. Ticks: ${get(TICKS) ?? "0"}; ` +
    `last tick: ${get(TICK) ?? "none"}`
  );
}

/** Launches the command `text` names, as "Launch" does, answering what it shows. */
function launchNamed(text: string | null | undefined): string {
  if (text == null) {
    throw new Error(
      "Launch needs the command to launch: send it `report`, `background report` or " +
        "`<package identity>#report`",
    );
  }
  const background = text.startsWith("background ");
  const named = background ? text.slice("background ".length).trim() : text;
  const at = named.lastIndexOf("#");
  const target: CommandRef =
    at < 0
      ? { source: null, command: named }
      : { source: named.slice(0, at), command: named.slice(at + 1) };
  try {
    launch(target, background ? "background" : "user-initiated", [], CONTEXT);
  } catch (error) {
    const payload = (error as { payload?: unknown }).payload;
    throw new Error(typeof payload === "string" ? payload : String(error));
  }
  return background ? `Launched ${named} in the background` : `Launched ${named}`;
}

async function render(record: LaunchRecord): Promise<List> {
  return {
    title: "Launch record",
    items: [
      { id: "type", title: `Launch type: ${record.launchType}` },
      { id: "source", title: `Source: ${record.source}` },
      { id: "fallback-text", title: `Fallback text: ${record.fallbackText ?? "none"}` },
      { id: "context", title: `Context: ${record.context ?? "none"}` },
    ],
  };
}

async function run(id: string, record: LaunchRecord): Promise<void> {
  let done: string;
  switch (id) {
    case "report":
      // Resolving with a value, where `run` resolves with nothing, is a
      // crash, unlike throwing, which is an error the extension answers with.
      if (record.fallbackText === "crash") return null as unknown as void;
      done = report(record);
      break;
    case "tick":
      done = tick(record);
      break;
    case "last":
      done = last();
      break;
    case "launch":
      done = launchNamed(record.fallbackText);
      break;
    default:
      throw new Error(`unknown command: ${id}`);
  }
  // Nobody is there to see a background launch's toast.
  if (record.launchType !== "background") showToast({ title: done });
}

export const command: Command = { render, run };
