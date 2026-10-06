// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's actions sample in TypeScript: a list whose items carry several
// actions (#137). Items, actions, sections, shortcuts and answers match the
// Rust actions sample (guests/sample-actions) and the JavaScript one.
// "Alpha note" has nine actions, in an untitled section and the "Edit",
// "Share" and "Danger" sections: Enter runs "Open", Ctrl+Enter "Copy" and
// Ctrl+Shift+Enter "Rename", and Ctrl+K lists them all. Its shortcuts show
// the rules Pane binds them by: one per system ("Reveal"), one that is
// Pane's own Ctrl+K and so is never bound ("Open Menu"), one that Pane
// leaves free until the user gives one of its keys Ctrl+Shift+Y
// ("Archive"), and a destructive "Delete". "Beta note" has one action, so
// Ctrl+Enter does nothing there, and "Gamma note" has none, so it cannot be
// activated. Every action answers its title and the item's
// ("Open: Alpha note").
import type { Action, Command, CustomView, List, Shortcut } from "@pane/extension";

/** The action titled `title` of the item titled `item`: it answers both. */
function action(
  title: string,
  item: string,
  more: { section?: string; style?: "destructive"; shortcut?: Shortcut } = {},
): Action {
  return { title, onAction: async () => `${title}: ${item}`, ...more };
}

const ALPHA = "Alpha note";
const BETA = "Beta note";

export const command: Command = {
  async render(): Promise<List> {
    return {
      title: "Actions sample",
      items: [
        {
          id: "alpha",
          title: ALPHA,
          subtitle: "Several actions in sections, with shortcuts",
          actions: [
            action("Open", ALPHA),
            action("Copy", ALPHA),
            action("Rename", ALPHA, {
              section: "Edit",
              shortcut: { modifiers: ["ctrl"], key: "r" },
            }),
            action("Duplicate", ALPHA, {
              section: "Edit",
              shortcut: { modifiers: ["ctrl"], key: "d" },
            }),
            action("Archive", ALPHA, {
              section: "Edit",
              shortcut: { modifiers: ["ctrl", "shift"], key: "y" },
            }),
            action("Copy Link", ALPHA, {
              section: "Share",
              shortcut: { modifiers: ["ctrl", "shift"], key: "c" },
            }),
            action("Reveal", ALPHA, {
              section: "Share",
              shortcut: {
                windows: { modifiers: ["ctrl", "shift"], key: "e" },
                macos: { modifiers: ["cmd", "shift"], key: "r" },
                linux: { modifiers: ["ctrl", "shift"], key: "l" },
              },
            }),
            action("Open Menu", ALPHA, {
              section: "Share",
              shortcut: { modifiers: ["ctrl"], key: "k" },
            }),
            action("Delete", ALPHA, {
              section: "Danger",
              style: "destructive",
              shortcut: { modifiers: ["ctrl"], key: "x" },
            }),
          ],
        },
        { id: "beta", title: BETA, subtitle: "One action", actions: [action("Open", BETA)] },
        { id: "gamma", title: "Gamma note", subtitle: "No actions" },
      ],
    };
  },

  async submitForm(itemId: string): Promise<string> {
    // A rejected submitForm is a message about the whole form.
    throw new Error(`The actions sample has no forms: ${itemId}`);
  },

  async openView(itemId: string): Promise<CustomView> {
    throw new Error(`The actions sample has no custom views: ${itemId}`);
  },
};
