// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's arguments sample in TypeScript: no-view commands that ask for typed
// values before they run (`"arguments"` in `pane.json`), all served by one
// component. Each receives the values in its launch record, by name; an
// optional argument left empty is absent. Commands, answers and errors
// match the Rust arguments sample (guests/sample-arguments) and the
// JavaScript one.
//
// - "Greet" has three arguments: a required text (`name`), an optional
//   password (`secret`) and a dropdown (`tone`). It answers which values it
//   was given (only the length of the secret, which it never shows or
//   keeps), where it was launched from and the fallback text, counting its
//   runs. Its first argument is text and the others are optional, so it may
//   be a fallback: text sent to it fills `name`.
// - "Stamp" has one required text argument (`label`), for launches with no
//   fields of their own (a global hotkey, a quick slot). It answers the
//   label and keeps it, with how it was launched, for "Relay last".
// - "Relay" launches the command of this package its text names, with the
//   arguments it lists: `stamp label=x`, or `background stamp label=x` in
//   the background. Sent `last`, it answers what "Stamp" last kept.
import { launch } from "pane:extension/commands@0.1.0";
import { get, set } from "pane:extension/settings@0.1.0";
import type { ArgumentValue, Command, LaunchRecord } from "@pane/extension";

/** The settings key holding how many times "Greet" ran. */
const RUNS = "greet-runs";
/** The settings key holding the label "Stamp" last stamped, and how. */
const STAMP = "stamp";

/** The value of the argument `name` in `record`, if it has one. */
function argument(record: LaunchRecord, name: string): string | undefined {
  return record.arguments.find((given) => given.name === name)?.value;
}

/** What "Greet" answers for `record`, counting the run. */
function greet(record: LaunchRecord): string {
  const saved = get(RUNS);
  const counted = saved === null ? 0 : Number.parseInt(saved, 10);
  const runs = (Number.isNaN(counted) ? 0 : counted) + 1;
  set(RUNS, String(runs));
  const given = record.arguments.map(({ name, value }) =>
    name === "secret" ? `secret (${Array.from(value).length} characters)` : `${name}=${value}`,
  );
  return (
    `Greet run ${runs} from ${record.source}: ` +
    `${given.length === 0 ? "nothing" : given.join(", ")}; ` +
    `fallback text: ${record.fallbackText ?? "none"}`
  );
}

/** What "Stamp" answers for `record`, keeping what it stamped. */
function stamp(record: LaunchRecord): string {
  const label = argument(record, "label");
  if (label === undefined) throw new Error("Stamp has no label");
  set(STAMP, `${label} from ${record.source}, ${record.launchType}`);
  return `Stamped ${label} from ${record.source}`;
}

/**
 * Launches the command `text` names with the arguments it lists, as "Relay"
 * does, or answers what "Stamp" last kept.
 */
function relay(text: string | null | undefined): string {
  if (text == null) {
    throw new Error(
      "Relay needs what to do: `last`, or a command and its arguments, such as " +
        "`background stamp label=x`",
    );
  }
  if (text === "last") return `Last stamp: ${get(STAMP) ?? "none"}`;
  const words = text.split(/\s+/).filter((word) => word !== "");
  const background = words[0] === "background";
  if (background) words.shift();
  const named = words.shift() ?? "";
  const given: ArgumentValue[] = words.map((word) => {
    const at = word.indexOf("=");
    return at < 0
      ? { name: word, value: "" }
      : { name: word.slice(0, at), value: word.slice(at + 1) };
  });
  try {
    launch({ source: null, command: named }, background ? "background" : "user-initiated", given, null);
  } catch (error) {
    const payload = (error as { payload?: unknown }).payload;
    throw new Error(typeof payload === "string" ? payload : String(error));
  }
  return background ? `Relayed ${named} in the background` : `Relayed ${named}`;
}

async function run(id: string, record: LaunchRecord): Promise<string> {
  switch (id) {
    case "greet":
      return greet(record);
    case "stamp":
      return stamp(record);
    case "relay":
      return relay(record.fallbackText);
    default:
      throw new Error(`unknown command: ${id}`);
  }
}

export const command: Command = { run };
