// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's clipboard history sample in TypeScript: the same contract as the
// Clipboard History default extension (guests/clipboard-history, Rust) and
// the JavaScript sample. Pane's host watches the clipboard and keeps the
// text the user copies for this package once they turn it on here, through
// `pane:extension/clipboard-history` (imported because package.json sets
// `"pane": { "clipboardHistory": true }`); the command only shows the
// history and the user's controls: turn on, pause, resume, turn off, how
// long items are kept (Pane deletes them then, whether the command runs or
// not), exclude a program, clear, turn off and delete, delete recent items,
// and Enter on an item copies it again or deletes it.
import * as history from "pane:extension/clipboard-history@0.1.0";
import type { Capture, Entry, HistoryStatus } from "pane:extension/clipboard-history@0.1.0";
import type { Command, CustomView, FieldValue, Form, Item, List } from "@pane/extension";

/** The longest title of a kept item, in characters. */
const TITLE_CHARS = 80;

const CAPTURES: Record<string, [Capture, string]> = {
  "turn-on": ["on", "Clipboard history is on"],
  pause: ["paused", "Clipboard history is paused"],
  resume: ["on", "Clipboard history is on again"],
  "turn-off": ["off", "Clipboard history is off"],
};
const INCLUDE = "include:";
const ENTRY = "entry:";

/** How long items can be kept, in seconds. */
const RETENTIONS = [3600, 86400, 7 * 86400, 30 * 86400, 90 * 86400];

/** How recent the items deleted together can be, in seconds, and what that is called. */
const RECENT: [number, string][] = [
  [900, "15 minutes"],
  [3600, "hour"],
  [86400, "day"],
];

/** The reason a host function failed with. */
const reason = (error: unknown): string => String((error as { payload?: unknown }).payload);

/** A host function's result, or an `Error` with the reason it failed. */
function host<T>(call: () => T): T {
  try {
    return call();
  } catch (error) {
    throw new Error(reason(error));
  }
}

const item = (id: string, title: string, subtitle: string): Item => ({ id, title, subtitle });

/** An item whose action is `act` with its id. */
const action = (id: string, title: string, subtitle: string): Item => ({
  ...item(id, title, subtitle),
  onAction: () => act(id),
});

const plural = (count: number, one: string, many: string): string =>
  count === 1 ? `1 ${one}` : `${count} ${many}`;

/** A time span such as "1 hour" or "7 days". */
function span(seconds: number): string {
  const [count, one, many]: [number, string, string] =
    seconds % 86400 === 0
      ? [seconds / 86400, "day", "days"]
      : seconds % 3600 === 0
        ? [seconds / 3600, "hour", "hours"]
        : seconds % 60 === 0
          ? [seconds / 60, "minute", "minutes"]
          : [seconds, "second", "seconds"];
  return plural(count, one, many);
}

/** A form with one choice field. */
function choiceForm(title: string, id: string, label: string, choices: [string, string][], submitLabel: string): Form {
  return {
    title,
    fields: [{ id, label, kind: { tag: "choice", val: choices.map(([id, label]) => ({ id, label })) } }],
    submitLabel,
  };
}

function toggle(status: HistoryStatus): Item {
  const kept = plural(status.items, "item", "items");
  const [id, title, subtitle]: [string, string, string] =
    status.capture === "off"
      ? [
          "turn-on",
          "Turn on clipboard history",
          "Off · Pane keeps nothing you copy until you turn it on. Once on, it keeps the text you " +
            "copy on this computer; nothing is sent anywhere",
        ]
      : status.capture === "on"
        ? ["pause", "Pause clipboard history", `On · ${kept} kept · Text you copy is kept on this computer`]
        : [
            "resume",
            "Resume clipboard history",
            `Paused · ${kept} kept · Nothing you copy is kept until you resume`,
          ];
  return action(id, title, status.problem ? `${status.problem} · ${subtitle}` : subtitle);
}

/** The first line of `text` with content, trimmed and at most `TITLE_CHARS` long. */
function titleOf(text: string): string {
  const line = text.split(/\r?\n/).map((line) => line.trim()).find((line) => line !== "") ?? "";
  const chars = [...line];
  return chars.length <= TITLE_CHARS ? line : chars.slice(0, TITLE_CHARS - 1).join("") + "…";
}

function age(seconds: number): string {
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
  if (seconds < 2 * 86400) return "1 day ago";
  return `${Math.floor(seconds / 86400)} days ago`;
}

function entryItem(entry: Entry): Item {
  const about = [age(entry.ageSeconds)];
  if (entry.source) about.push(`from ${entry.source}`);
  const lines = entry.text.split(/\r?\n/).length - (entry.text.endsWith("\n") ? 1 : 0);
  if (lines > 1) about.push(`${lines} lines`);
  about.push("Enter copies or deletes it");
  const title = titleOf(entry.text);
  return {
    ...item(`${ENTRY}${entry.id}`, title, about.join(" · ")),
    // Copying again comes first, so Enter twice copies.
    form: choiceForm(
      title,
      "action",
      "What to do with it",
      [
        ["copy", "Copy it again"],
        ["delete", "Delete it"],
      ],
      "OK",
    ),
  };
}

async function render(): Promise<List> {
  const status = host(history.status);
  const items: Item[] = [toggle(status)];
  if (status.capture !== "off") {
    items.push(
      action(
        "turn-off",
        "Turn off clipboard history",
        "Stops keeping what you copy; the kept items stay until you clear them",
      ),
    );
  }
  items.push({
    ...item(
      "retention",
      `Keep items for ${span(status.retentionSeconds)}`,
      "Older items are deleted, also while Pane is stopped or the extension is disabled · Enter changes it",
    ),
    form: choiceForm(
      "Keep clipboard history items for",
      "retention",
      "Keep each item for",
      // A form starts on its first choice, so the retention now comes first:
      // submitting the form unchanged changes nothing.
      [status.retentionSeconds, ...RETENTIONS.filter((seconds) => seconds !== status.retentionSeconds)].map(
        (seconds) => [String(seconds), span(seconds)],
      ),
      "Keep",
    ),
  });
  const excluded = status.excluded.length === 0 ? "None excluded" : `${status.excluded.length} excluded`;
  items.push({
    ...item("exclude", "Exclude a program", `Text copied from it is never kept · ${excluded}`),
    form: {
      title: "Exclude a program",
      fields: [
        {
          id: "program",
          label: "Program file name",
          kind: { tag: "text", val: { placeholder: "KeePass.exe" } },
        },
      ],
      submitLabel: "Exclude",
    },
  });
  for (const program of status.excluded) {
    items.push(
      action(`${INCLUDE}${program}`, `Stop excluding ${program}`, `Text copied from ${program} is not kept`),
    );
  }
  const entries = host(history.entries);
  if (entries.length > 0) {
    items.push(
      action(
        "clear",
        "Clear clipboard history",
        `Deletes the ${plural(status.items, "item", "items")} kept; whether history is kept does not change`,
      ),
    );
    if (status.capture !== "off") {
      items.push(
        action(
          "turn-off-and-clear",
          "Turn off and delete clipboard history",
          `Deletes the ${plural(status.items, "item", "items")} kept and keeps nothing you copy from now on`,
        ),
      );
    }
    items.push({
      ...item("delete-recent", "Delete recent items", "Deletes what you copied in the last 15 minutes, hour or day"),
      form: choiceForm(
        "Delete recent clipboard history items",
        "since",
        "Copied in the last",
        RECENT.map(([seconds, label]) => [String(seconds), label]),
        "Delete",
      ),
    });
  }
  items.push(...entries.map(entryItem));
  if (entries.length === 0 && status.capture === "on") {
    items.push(action("empty", "Nothing kept yet", "Text you copy from now on is listed here"));
  }
  return { title: "Clipboard history (TypeScript)", items };
}

/** Runs the action of the item `itemId`. */
async function act(itemId: string): Promise<string> {
  const wanted = CAPTURES[itemId];
  if (wanted) {
    host(() => history.setCapture(wanted[0]));
    return wanted[1];
  }
  if (itemId === "clear") {
    return `Deleted ${plural(host(history.clear), "kept item", "kept items")}`;
  }
  if (itemId === "turn-off-and-clear") {
    return `Clipboard history is off; deleted ${plural(host(history.turnOffAndClear), "kept item", "kept items")}`;
  }
  if (itemId === "empty") return "Nothing is kept yet";
  if (itemId.startsWith(INCLUDE)) {
    const program = itemId.slice(INCLUDE.length);
    const excluded = host(history.status).excluded.filter((excluded) => excluded !== program);
    host(() => history.setExcluded(excluded));
    return `Text copied from ${program} is kept again`;
  }
  if (itemId.startsWith(ENTRY)) {
    host(() => history.copy(itemId.slice(ENTRY.length)));
    return "Copied to the clipboard";
  }
  throw new Error(`unknown item: ${itemId}`);
}

/** A host function's result, or a form error with the reason it failed. */
function forForm<T>(call: () => T): T {
  try {
    return call();
  } catch (error) {
    throw { message: reason(error) };
  }
}

async function submitForm(itemId: string, values: FieldValue[]): Promise<string> {
  const value = (id: string): string => (values.find((value) => value.id === id)?.value ?? "").trim();
  if (itemId.startsWith(ENTRY)) {
    const id = itemId.slice(ENTRY.length);
    if (value("action") === "delete") {
      if (forForm(() => history.deleteItems([id])) === 0) throw { message: "That item is no longer kept" };
      return "Deleted the kept item";
    }
    forForm(() => history.copy(id));
    return "Copied to the clipboard";
  }
  if (itemId === "retention") {
    const seconds = Number(value("retention"));
    const before = forForm(history.status).items;
    forForm(() => history.setRetention(seconds));
    const deleted = before - forForm(history.status).items;
    const kept = `Items are kept for ${span(seconds)}`;
    return deleted > 0 ? `${kept}; deleted ${plural(deleted, "older item", "older items")}` : kept;
  }
  if (itemId === "delete-recent") {
    const seconds = Number(value("since"));
    const ids = forForm(history.entries)
      .filter((entry) => entry.ageSeconds < seconds)
      .map((entry) => entry.id);
    return `Deleted ${plural(forForm(() => history.deleteItems(ids)), "kept item", "kept items")}`;
  }
  if (itemId !== "exclude") throw { message: `unknown form: ${itemId}` };
  const program = value("program");
  const excluded = host(history.status).excluded;
  try {
    history.setExcluded([...excluded, program]);
  } catch (error) {
    throw { field: "program", message: reason(error) };
  }
  return `Text copied from ${program} is not kept`;
}

async function openView(itemId: string): Promise<CustomView> {
  throw new Error(`unknown view: ${itemId}`);
}

/**
 * A callback no item's action names runs as the action of that id, so a
 * kept item's id (whose item opens a form) still copies it again.
 */
async function runSearchResult(id: string): Promise<string> {
  return act(id);
}

export const command: Command = { render, runSearchResult, submitForm, openView };
