// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's preferences sample in TypeScript: a package that declares
// preferences of every type in `pane.json`, for the whole extension and for
// single commands, and commands that read their effective values with
// `getPreferenceValues` from `@pane/extension/preferences`, as interfaces
// of their own. Commands and toasts match the Rust preferences sample
// (guests/sample-preferences) and the JavaScript one.
//
// - The package declares an API key (a password, required, no default),
//   units (a dropdown, required, with a default), a greeting (text,
//   optional) and "Verbose" (a checkbox). Until the API key is set every
//   command needs setup: Pane shows its Setup screen, with the package's
//   HELP.md, before the first run.
// - "Show preferences" is a view command that also declares a notes folder
//   (a folder, required), a notes file (a file) and an editor (an
//   application): its list shows every value it received.
// - "Report preferences" is a no-view command that also declares "Loud" (a
//   checkbox): it shows in a toast where it was launched from and its
//   values, shouted when Loud is on.
// - "Tick" runs every minute on its own schedule, in the background, once
//   the package is set up, counting its runs; "Last tick" shows the count
//   in a toast.
//
// A command launched in the background (a schedule's run) does its work but
// shows no toast.
import { showToast } from "@pane/extension/feedback";
import { getPreferenceValues } from "@pane/extension/preferences";
import { get, set } from "pane:extension/settings@0.1.0";
import type { Command, LaunchRecord, List } from "@pane/extension";

/** The settings key holding how many times "Tick" ran. */
const TICKS = "ticks";

/** The package's preferences, which every command receives. */
interface PackagePreferences {
  apiKey: string;
  units: "metric" | "imperial";
  greeting?: string;
  verbose: boolean;
}

/** What "Show preferences" receives: its package's and its own. */
interface ShowPreferences extends PackagePreferences {
  folder: string;
  notes?: string;
  editor?: string;
}

/** What "Report preferences" receives: its package's and its own. */
interface ReportPreferences extends PackagePreferences {
  loud: boolean;
}

/** The number of characters of `text`, as Rust counts them. */
function characters(text: string): number {
  return Array.from(text).length;
}

/** What "Report preferences" shows, launched as `record` says. */
function report(record: LaunchRecord): string {
  const values = getPreferenceValues<ReportPreferences>();
  const answer =
    `Report from ${record.source}: API key of ${characters(values.apiKey)} characters; units: ${values.units}; ` +
    `greeting: ${values.greeting ?? "none"}; verbose: ${values.verbose}`;
  return values.loud ? answer.toUpperCase() : answer;
}

/** What "Tick" would show, counting the run. */
function tick(): string {
  const saved = get(TICKS);
  const counted = saved === null ? 0 : Number.parseInt(saved, 10);
  const ticks = (Number.isNaN(counted) ? 0 : counted) + 1;
  set(TICKS, String(ticks));
  return `Ticked ${ticks} times`;
}

/** What "Last tick" shows: how many times "Tick" ran. */
function last(): string {
  return `Ticks: ${get(TICKS) ?? "0"}`;
}

export const command: Command = {
  async render(): Promise<List> {
    const values = getPreferenceValues<ShowPreferences>();
    return {
      title: "Preferences",
      items: [
        { id: "api-key", title: `API key: ${characters(values.apiKey)} characters` },
        { id: "units", title: `Units: ${values.units}` },
        { id: "greeting", title: `Greeting: ${values.greeting ?? "none"}` },
        { id: "verbose", title: `Verbose: ${values.verbose}` },
        { id: "folder", title: `Notes folder: ${values.folder}` },
        { id: "notes", title: `Notes file: ${values.notes ?? "none"}` },
        { id: "editor", title: `Editor: ${values.editor ?? "none"}` },
      ],
    };
  },
  async run(id: string, record: LaunchRecord): Promise<void> {
    let done: string;
    switch (id) {
      case "report":
        done = report(record);
        break;
      case "tick":
        done = tick();
        break;
      case "last":
        done = last();
        break;
      default:
        throw new Error(`unknown command: ${id}`);
    }
    // Nobody is there to see a background launch's toast.
    if (record.launchType !== "background") showToast({ title: done });
  },
};
