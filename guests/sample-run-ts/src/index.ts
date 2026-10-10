// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's Run sample in TypeScript: a command that runs what Windows' Run
// dialog (Win+R) runs through `pane:extension/run`, sharing the Run
// dialog's own history, completing the command line as it is typed and
// running one in a terminal. Answers and errors match the Rust sample
// (guests/sample-run) and the JavaScript one.
import type {
  Command,
  CommandSearch,
  LaunchRecord,
  LaunchType,
  List,
  SearchResult,
} from "@pane-app/extension";
import { showToast } from "@pane-app/extension/feedback";
import { getPreferenceValues } from "@pane-app/extension/preferences";
import { run as runProgram } from "@pane-app/extension/programs";
import type { Completion, CompletionSource } from "pane:extension/run@0.1.0";
import {
  completions,
  deleteFromHistory,
  history,
  run,
  terminal,
} from "pane:extension/run@0.1.0";

/** What the no-view commands say when they are sent no command line. */
const NOTHING =
  "Run was sent no command line: give it an alias or make it a fallback in Settings, then type " +
  "a command line in root search";

/** The prefix of a history entry's item id, followed by its line. */
const ENTRY = "entry:";
/** The prefix of a search result's item id, followed by the line Enter runs. */
const LINE = "line:";

/** The command with id `in-terminal`, which runs one in a terminal. */
const IN_TERMINAL = "in-terminal";
/** The command with id `with-completions`, whose search field holds the line. */
const WITH_COMPLETIONS = "with-completions";

/** Calls a host function, turning its error into a plain message. */
function host<T>(call: () => T): T {
  try {
    return call();
  } catch (error) {
    const payload = (error as { payload?: unknown }).payload;
    if (payload !== null && typeof payload === "object" && "tag" in payload) {
      throw new Error(String((payload as { val?: unknown }).val));
    }
    throw typeof payload === "string" ? new Error(payload) : error;
  }
}

/**
 * Runs the command line `text` was sent, elevated when `elevated`,
 * showing a toast that says what ran (a launch in the background shows
 * nothing).
 */
async function runLine(
  text: string | null | undefined,
  elevated: boolean,
  launchType: LaunchType,
): Promise<void> {
  const line = text?.trim();
  if (line == null || line === "") throw new Error(NOTHING);
  host(() => run(line, elevated));
  if (launchType !== "background") {
    showToast({
      title: elevated ? `Ran ${line} as an administrator` : `Ran ${line}`,
    });
  }
}

/** The shell the terminal preference chooses, as it runs a command line. */
interface Shell {
  program: string;
  arguments: string[];
}

/**
 * The shell the package's preference chooses for the terminal: PowerShell,
 * the preference's default, or the Command Prompt.
 */
function shellOf(): Shell {
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
 */
async function runInTerminal(
  text: string | null | undefined,
  launchType: LaunchType,
): Promise<void> {
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

/** Removes `line` from the shared history, saying so. */
async function deleteLine(line: string): Promise<void> {
  host(() => deleteFromHistory(line));
  showToast({ title: "Deleted from Run's history" });
}

/** What a completion's source says, as its row's subtitle. */
function subtitleOf(source: CompletionSource): string {
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

export const command: Command = {
  async run(id: string, launch: LaunchRecord): Promise<void> {
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

  async render(launch: LaunchRecord): Promise<List> {
    if (launch.command === WITH_COMPLETIONS) {
      return {
        title: "Run with Completions",
        items: [
          {
            id: "hint",
            title: "Type a command line",
            subtitle: "What you ran and installed completes it as you type",
            onAction: async () => {
              showToast({ title: "Type a command line to run it" });
            },
          },
        ],
      };
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

  async runSearchResult(id: string): Promise<void> {
    if (id.startsWith(ENTRY)) {
      await runLine(id.slice(ENTRY.length), false, "user-initiated");
    } else if (id.startsWith(LINE)) {
      await runLine(id.slice(LINE.length), false, "user-initiated");
    } else {
      throw new Error(`unknown item: ${id}`);
    }
  },

  async submitForm() {
    throw { message: "this sample has no forms" };
  },

  async openView() {
    throw new Error("this sample has no custom views");
  },
};

export const commandSearch: CommandSearch = {
  // The completions for the text typed in "Run with Completions"' field,
  // with the text's own row first: Enter runs the text as typed, then
  // each completion's line.
  async search(command: string, query: string): Promise<SearchResult[]> {
    if (command !== WITH_COMPLETIONS) throw new Error(`unknown command: ${command}`);
    const text = query.trim();
    if (text === "") return [];
    const found = host(() => completions(text));
    return [
      {
        id: `${LINE}${text}`,
        title: `Run “${text}”`,
        subtitle: "Enter runs it as typed",
      },
      ...found.map((completion: Completion) => ({
        id: `${LINE}${completion.line}`,
        title: completion.line,
        subtitle: subtitleOf(completion.source),
      })),
    ];
  },
};
