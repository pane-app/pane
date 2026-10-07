// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's clipboard history sample in JavaScript: the same contract as the
// Clipboard History default extension (guests/clipboard-history, Rust) and
// the TypeScript sample. Pane's host watches the clipboard and keeps the
// text the user copies for this package once they turn it on here, through
// `pane:extension/clipboard-history` (imported because package.json sets
// `"pane": { "clipboardHistory": true }`); the command only shows the
// history and the user's controls: turn on, pause, resume, turn off, how
// long items are kept (Pane deletes them then, whether the command runs or
// not), exclude a program, clear, turn off and delete, delete recent items.
// Each kept item has three actions: Paste (Enter) pastes it into the
// application that was in front, or, where Pane cannot paste yet, copies it
// and says so in a HUD; Copy copies it again; Delete, destructive and last,
// deletes it.
// @ts-check
import { closeMainWindow, showHUD, showToast } from "@pane/extension/feedback";
import { NotAvailableError, PASTE_FALLBACK, paste } from "@pane/extension/system";
import * as history from "pane:extension/clipboard-history@0.1.0";

/** The longest title of a kept item, in characters. */
const TITLE_CHARS = 80;

/** @type {Record<string, [history.Capture, string]>} */
const CAPTURES = {
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
/** @type {[number, string][]} */
const RECENT = [
  [900, "15 minutes"],
  [3600, "hour"],
  [86400, "day"],
];

/**
 * A host function's result, or an `Error` with the reason it failed.
 * @template T
 * @param {() => T} call
 * @returns {T}
 */
function host(call) {
  try {
    return call();
  } catch (error) {
    throw new Error(String(/** @type {any} */ (error).payload));
  }
}

/**
 * A host function's result, or a form error with the reason it failed.
 * @template T
 * @param {() => T} call
 * @returns {T}
 */
function forForm(call) {
  try {
    return call();
  } catch (error) {
    throw { message: String(/** @type {any} */ (error).payload) };
  }
}

/**
 * @param {string} id
 * @param {string} title
 * @param {string} subtitle
 * @returns {import("@pane/extension").Item}
 */
const item = (id, title, subtitle) => ({ id, title, subtitle });

/**
 * An item whose action is `act` with its id.
 * @param {string} id
 * @param {string} title
 * @param {string} subtitle
 * @returns {import("@pane/extension").Item}
 */
const action = (id, title, subtitle) => ({ ...item(id, title, subtitle), onAction: () => act(id) });

/**
 * @param {number} count
 * @param {string} one
 * @param {string} many
 */
const plural = (count, one, many) => (count === 1 ? `1 ${one}` : `${count} ${many}`);

/**
 * A time span such as "1 hour" or "7 days".
 * @param {number} seconds
 */
function span(seconds) {
  /** @type {[number, string, string]} */
  const [count, one, many] =
    seconds % 86400 === 0
      ? [seconds / 86400, "day", "days"]
      : seconds % 3600 === 0
        ? [seconds / 3600, "hour", "hours"]
        : seconds % 60 === 0
          ? [seconds / 60, "minute", "minutes"]
          : [seconds, "second", "seconds"];
  return plural(count, one, many);
}

/**
 * A form with one choice field.
 * @param {string} title
 * @param {string} id
 * @param {string} label
 * @param {[string, string][]} choices
 * @param {string} submitLabel
 * @returns {import("@pane/extension").Form}
 */
function choiceForm(title, id, label, choices, submitLabel) {
  return {
    title,
    fields: [{ id, label, kind: { tag: "choice", val: choices.map(([id, label]) => ({ id, label })) } }],
    submitLabel,
  };
}

/** @param {history.HistoryStatus} status */
function toggle(status) {
  const kept = plural(status.items, "item", "items");
  /** @type {[string, string, string]} */
  const [id, title, subtitle] =
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

/**
 * The first line of `text` with content, trimmed and at most `TITLE_CHARS`
 * long.
 * @param {string} text
 */
function titleOf(text) {
  const line = text.split(/\r?\n/).map((line) => line.trim()).find((line) => line !== "") ?? "";
  const chars = [...line];
  return chars.length <= TITLE_CHARS ? line : chars.slice(0, TITLE_CHARS - 1).join("") + "…";
}

/** @param {number} seconds */
function age(seconds) {
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
  if (seconds < 2 * 86400) return "1 day ago";
  return `${Math.floor(seconds / 86400)} days ago`;
}

/**
 * @param {history.Entry} entry
 * @returns {import("@pane/extension").Item}
 */
function entryItem(entry) {
  const about = [age(entry.ageSeconds)];
  if (entry.source) about.push(`from ${entry.source}`);
  const lines = entry.text.split(/\r?\n/).length - (entry.text.endsWith("\n") ? 1 : 0);
  if (lines > 1) about.push(`${lines} lines`);
  about.push("Enter pastes it");
  const title = titleOf(entry.text);
  return {
    ...item(`${ENTRY}${entry.id}`, title, about.join(" · ")),
    actions: [
      { title: "Paste", onAction: () => pasteEntry(entry.id, entry.text) },
      { title: "Copy", onAction: () => copyEntry(entry.id) },
      { title: "Delete", style: "destructive", onAction: () => deleteEntry(entry.id) },
    ],
  };
}

/**
 * Paste: pastes the kept item `id`, whose text is `text`, into the
 * application that was in front before Pane, which closes the window; where
 * Pane cannot paste yet, copies it again instead (as Copy does), closes the
 * window and says so in a HUD.
 * @param {string} id
 * @param {string} text
 * @returns {Promise<void>}
 */
async function pasteEntry(id, text) {
  try {
    paste(text);
  } catch (error) {
    if (!(error instanceof NotAvailableError)) throw error;
    host(() => history.copy(id));
    closeMainWindow();
    showHUD(PASTE_FALLBACK);
  }
}

/**
 * Copy: puts the kept item `id` on the clipboard again, closes the window
 * and says so in a HUD.
 * @param {string} id
 * @returns {Promise<void>}
 */
async function copyEntry(id) {
  host(() => history.copy(id));
  closeMainWindow();
  showHUD("Copied to Clipboard");
}

/**
 * Delete: deletes the kept item `id`; one no longer kept is an error.
 * @param {string} id
 * @returns {Promise<void>}
 */
async function deleteEntry(id) {
  if (host(() => history.deleteItems([id])) === 0) throw new Error("That item is no longer kept");
  showToast({ title: "Deleted the kept item" });
}

/**
 * Runs the action of the item `itemId`, showing a toast with what it did.
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
  throw new Error(`unknown item: ${itemId}`);
}

/** @type {import("@pane/extension").Command} */
export const command = {
  async render() {
    const status = host(history.status);
    const items = [toggle(status)];
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
    return { title: "Clipboard history (JavaScript)", items };
  },

  // A callback no item's action names: an action of a list drawn before,
  // whose item is gone now.
  async runSearchResult(id) {
    await act(id);
  },

  async submitForm(itemId, values) {
    /** @param {string} id */
    const value = (id) => (values.find((value) => value.id === id)?.value ?? "").trim();
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
      throw { field: "program", message: String(/** @type {any} */ (error).payload) };
    }
    return `Text copied from ${program} is not kept`;
  },

  async openView(itemId) {
    throw new Error(`unknown view: ${itemId}`);
  },
};
