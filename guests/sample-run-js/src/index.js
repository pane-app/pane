// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's Run sample in JavaScript: a command that runs what Windows' Run
// dialog (Win+R) runs through `pane:extension/run`, sharing the Run
// dialog's own history, completing the command line as it is typed and
// running one in a terminal. Answers and errors match the Rust sample
// (guests/sample-run) and the TypeScript one.
// @ts-check
import { showToast } from "@pane-app/extension/feedback";
import { getPreferenceValues } from "@pane-app/extension/preferences";
import { run as runProgram } from "@pane-app/extension/programs";
import { List, createView, useState } from "@pane-app/extension/view";
import { jsxs } from "@pane-app/extension/jsx-runtime";
import {
  completions,
  deleteFromHistory,
  history,
  run,
  terminal,
} from "pane:extension/run@0.2.0";

/** What the no-view commands say when they are sent no command line. */
const NOTHING =
  "Run was sent no command line: give it an alias or make it a fallback in Settings, then type " +
  "a command line in root search";

/** The prefix of a search result's item id, followed by the line Enter runs. */
const LINE = "line:";

/** The command with id `in-terminal`, which runs one in a terminal. */
const IN_TERMINAL = "in-terminal";
/** The command with id `with-completions`, whose search field holds the line. */
const WITH_COMPLETIONS = "with-completions";

/**
 * Calls a host function, turning its error into a plain message.
 * @template T
 * @param {() => T} call
 * @returns {T}
 */
function host(call) {
  try {
    return call();
  } catch (error) {
    const payload = /** @type {{ payload?: unknown }} */ (error).payload;
    if (payload !== null && typeof payload === "object" && "tag" in payload) {
      throw new Error(String(/** @type {{ val?: unknown }} */ (payload).val));
    }
    throw typeof payload === "string" ? new Error(payload) : error;
  }
}

/**
 * Runs the command line `text` was sent, elevated when `elevated`,
 * showing a toast that says what ran (a launch in the background shows
 * nothing).
 * @param {string | null | undefined} text
 * @param {boolean} elevated
 * @param {import("@pane-app/extension").LaunchType} launchType
 * @returns {Promise<void>}
 */
async function runLine(text, elevated, launchType) {
  const line = text?.trim();
  if (line == null || line === "") throw new Error(NOTHING);
  host(() => run(line, elevated));
  if (launchType !== "background") {
    showToast({
      title: elevated ? `Ran ${line} as an administrator` : `Ran ${line}`,
    });
  }
}

/**
 * The shell the terminal preference chooses, as it runs a command line:
 * PowerShell, the preference's default, or the Command Prompt.
 * @returns {{ program: string, arguments: string[] }}
 */
function shellOf() {
  const { shell } = getPreferenceValues();
  return shell === "cmd"
    ? { program: "cmd", arguments: ["/d", "/k"] }
    : { program: "powershell", arguments: ["-NoExit", "-Command"] };
}

/**
 * Runs the command line `text` was sent in a terminal, showing a toast
 * that says what ran (a launch in the background shows nothing). With
 * Windows Terminal installed, its new tab runs the command line through
 * ADR 0033's run-program host function, the shell the preference
 * chooses: the terminal owns the shell, so what the command wrote shows
 * in the tab and stays while the user reads it (a tab records nothing in
 * the Run dialog's history; it is a terminal, not a run). Without
 * Windows Terminal, the shell's own window is opened by the run host
 * function's own open: a program the run-program function starts belongs
 * to the call that started it (ADR 0033) and its streams are Pane's, so
 * its console window would show nothing and close when the run closed
 * its input — the system open gives the shell a window of its own that
 * stays, and that run records the shell's command line in the Run
 * dialog's history, as the run function does.
 * @param {string | null | undefined} text
 * @param {import("@pane-app/extension").LaunchType} launchType
 * @returns {Promise<void>}
 */
async function runInTerminal(text, launchType) {
  const line = text?.trim();
  if (line == null || line === "") throw new Error(NOTHING);
  const shell = shellOf();
  const where = host(() => terminal());
  if (where != null) {
    // A new tab in the running Windows Terminal with the chosen shell.
    await runProgram(where, ["-w", "0", "new-tab", shell.program, ...shell.arguments, line]);
  } else {
    // The shell's own window, as the system opens it.
    host(() => run(`${shell.program} ${[...shell.arguments, line].join(" ")}`, false));
  }
  if (launchType !== "background") {
    showToast({ title: `Ran ${line} in the terminal` });
  }
}

/**
 * Removes `line` from the shared history, saying so.
 * @param {string} line
 * @returns {Promise<void>}
 */
async function deleteLine(line) {
  host(() => deleteFromHistory(line));
  showToast({ title: "Deleted from Run's history" });
}

/**
 * What a completion's source says, as its row's subtitle.
 * @param {import("pane:extension/run@0.2.0").CompletionSource} source
 * @returns {string}
 */
function subtitleOf(source) {
  switch (source) {
    case "history":
      return "Run history";
    case "app-path":
      return "Program in App Paths";
    case "search-path":
      return "Program on the search path";
    case "applet":
      return "Control Panel applet";
    case "console":
      return "Management console";
    case "scheme":
      return "Registered scheme";
    default:
      return "Environment variable";
  }
}

/** @type {import("@pane-app/extension").Command} */
export const command = {
  async run(id, launch) {
    switch (id) {
      case "run":
        await runLine(launch.fallbackText, false, launch.launchType);
        return;
      case "elevated":
        await runLine(launch.fallbackText, true, launch.launchType);
        return;
      case IN_TERMINAL:
        await runInTerminal(launch.fallbackText, launch.launchType);
        return;
      case WITH_COMPLETIONS:
        throw new Error(`\`${WITH_COMPLETIONS}\` opens a screen; it has no run entry point`);
      case "history":
        throw new Error("`history` opens a screen; it has no run entry point");
      default:
        throw new Error(`unknown command: ${id}`);
    }
  },

  async render(launch) {
    if (launch.command === WITH_COMPLETIONS) {
      throw new Error(`\`${WITH_COMPLETIONS}\` opens a designed view; it has no list`);
    }
    const entries = host(history);
    if (entries.length === 0) {
      return {
        title: "Run History",
        items: [
          {
            id: "empty",
            title: "Nothing has been run yet",
            subtitle: "What you run in Pane or the Run dialog appears here",
          },
        ],
      };
    }
    return {
      title: "Run History",
      items: entries.map((line) => ({
        id: `entry:${line}`,
        title: line,
        subtitle: "Enter runs it again",
        onAction: () => runLine(line, false, "user-initiated"),
        actions: [
          {
            title: "Delete",
            style: "destructive",
            onAction: () => deleteLine(line),
          },
        ],
      })),
    };
  },

  async openView(command) {
    if (command !== WITH_COMPLETIONS) {
      throw new Error(`unknown command: ${command}`);
    }
    return createView(Completions);
  },
};

/** The view "Run with Completions" opens (#240): a List that handles its
 * search itself, the completions of the text typed as its items — the
 * text's own row first, Enter running it as typed, then each completion's
 * line. What the view runs says what it did in a toast, and the list is
 * drawn again after it. */
function Completions() {
  const [text, setText] = useState("");
  const trimmed = text.trim();
  const items = [];
  if (trimmed === "") {
    items.push(
      jsxs(List.Item, {
        key: "hint",
        title: "Type a command line",
        subtitle: "What you ran and installed completes it as you type",
        onClick: async () => {
          showToast({ title: "Type a command line to run it" });
        },
        children: ["Type a command line"],
      }),
    );
  } else {
    items.push(
      jsxs(List.Item, {
        key: `${LINE}${trimmed}`,
        title: `Run “${trimmed}”`,
        subtitle: "Enter runs it as typed",
        onClick: () => runLine(trimmed, false, "user-initiated"),
        children: [`Run “${trimmed}”`],
      }),
    );
    for (const completion of host(() => completions(trimmed))) {
      items.push(
        jsxs(List.Item, {
          key: `${LINE}${completion.line}`,
          title: completion.line,
          subtitle: subtitleOf(completion.source),
          onClick: () => runLine(completion.line, false, "user-initiated"),
          children: [completion.line],
        }),
      );
    }
  }
  return jsxs(List, {
    navigationTitle: "Run with Completions",
    searchPlaceholder: "Type a command line…",
    onSearchText: (/** @type {string} */ typed) => setText(typed),
    children: items,
  });
}
