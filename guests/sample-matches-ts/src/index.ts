// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's sample of a command's `when` and `matches` in `pane.json`, in
// TypeScript (#195): when root search lists a command — with a blank query,
// only while searching, or both — and what it matches: its title as usual,
// only URL-like queries, or only path-like queries. Four no-view commands
// share this component: "Hear an Address" (`url`, `"matches": "url"`),
// listed only for a typed web address, which root search parses (inferring
// `https://` before a bare domain) and sends as the launch record's
// fallback text; "Hear a Path" (`path`, `"matches": "file-path"`), the same
// for a typed path, resolved (`~` to the home folder, `file://` taken
// off); "Blank Only" (`blank`, `"when": "blank"`), listed only while
// nothing is typed; and "Searching Only" (`searching`, `"when":
// "searching"`), listed only while something is. Each answers the text it
// was sent with a toast, and toasts and errors match the Rust matches
// sample (guests/sample-matches) and the JavaScript one.
import type { Command, LaunchRecord } from "@pane-app/extension";
import { showToast } from "@pane-app/extension/feedback";

/** What every command says when it was sent no text. */
const NOTHING = "Heard nothing: a typed web address or path is what reaches these commands";

async function run(id: string, launch: LaunchRecord): Promise<void> {
  switch (id) {
    case "url":
    case "path":
    case "blank":
    case "searching":
      break;
    default:
      throw new Error(`unknown command: ${id}`);
  }
  const text = launch.fallbackText;
  const heard = text == null ? NOTHING : `Heard “${text}”`;
  if (launch.launchType !== "background") showToast({ title: heard });
}

export const command: Command = { run };
