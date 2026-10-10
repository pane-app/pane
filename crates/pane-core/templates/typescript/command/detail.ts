// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// The __NAME__ command: the detail of the text root search sends it, as
// the `detail` template's own command shows it. Added to the package by
// `pane-ext new command`; the entry file (`src/index.ts`) calls its
// `render`.
import type { LaunchRecord, List } from "@pane-app/extension";
import { showToast } from "@pane-app/extension/feedback";
import { copy } from "@pane-app/extension/system";

/** What the command shows when root search sent it nothing: it says how to
 *  send it something. */
const NOTHING =
  "Nothing yet: give this command an alias, or make it a fallback, then type into root search";

/** The command's detail: the text it was sent, its measures, and a copy
 *  action on its row. */
export async function render(launch: LaunchRecord): Promise<List> {
  const text = launch.fallbackText ?? "";
  return {
    title: "__TITLE__",
    items: [
      {
        id: "text",
        title: text === "" ? NOTHING : text,
        subtitle: "What root search sent, through an alias or as a fallback",
      },
      {
        id: "copy",
        title: "Copy the text",
        subtitle: "Puts what root search sent on the clipboard",
        onAction: async () => {
          copy(text);
          showToast({ title: "Copied" });
        },
      },
      {
        id: "characters",
        title: `${[...text].length} characters`,
        subtitle: "Counted as Unicode characters, not UTF-16 units",
      },
      { id: "words", title: `${text.split(/\s+/).filter(Boolean).length} words` },
    ],
  };
}
