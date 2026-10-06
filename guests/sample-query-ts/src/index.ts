// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Echo, the query sample, in TypeScript: a command that takes a query. It
// shows a toast with the text the user sends it from root search, through
// its alias ("ec hello" when the user gave it the alias "ec") or by choosing
// it as a fallback; Pane sends the text only when the user invokes it that
// way, as the fallback text of its launch record. Echo is a no-view command
// (`"mode": "no-view"`): it opens no screen, and root search stays as it was
// while its toast shows. Launched in the background, it shows nothing.
// Toasts and errors match the Rust query sample (guests/sample-query) and
// the JavaScript one. "fail" is refused, to show how an error looks;
// "crash" crashes on purpose, and three crashes within five minutes pause
// the extension.
import type { Command, LaunchRecord } from "@pane/extension";
import { showToast } from "@pane/extension/feedback";

/** What Echo says when it was sent no text. */
const NOTHING =
  "Echo heard nothing: give it an alias or make it a fallback in Manage extensions, then send " +
  "it text from root search";

async function run(id: string, launch: LaunchRecord): Promise<void> {
  if (id !== "echo") throw new Error(`unknown command: ${id}`);
  const text = launch.fallbackText;
  if (text === "fail") throw new Error("Echo refuses “fail”, to show how an error looks");
  // Resolving with a value, where `run` resolves with nothing, is a crash,
  // unlike throwing, which is an error the extension answers with.
  if (text === "crash") return null as unknown as void;
  const heard = text == null ? NOTHING : `Echo heard “${text}”`;
  if (launch.launchType !== "background") showToast({ title: heard });
}

export const command: Command = { run };
