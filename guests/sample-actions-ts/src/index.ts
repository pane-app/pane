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
//
// "Delta note" shows submenus (#140): "Open With…" gives its entries at once,
// in sections, two with shortcuts and a destructive one; "Move to List…"
// gives them when it opens, in a section that counts how many times it was
// asked ("Asked 1 time"); and "Tag…" fails when it opens ("The tags could
// not be loaded"). An entry answers what it did and the item's ("Open With
// Notepad: Delta note", "Move to Later: Delta note").
import type { Action, Command, CustomView, Item, List, Shortcut } from "@pane/extension";

/** What an action may say besides its title. */
type More = { section?: string; style?: "destructive"; shortcut?: Shortcut };

/** The action titled `title` of the item titled `item`: it answers both. */
function action(title: string, item: string, more: More = {}): Action {
  return answering(title, title, item, more);
}

/**
 * The action titled `title` of the item titled `item` that answers `said`
 * and the item's title.
 */
function answering(title: string, said: string, item: string, more: More = {}): Action {
  return { title, onAction: async () => `${said}: ${item}`, ...more };
}

const ALPHA = "Alpha note";
const BETA = "Beta note";
const DELTA = "Delta note";

/** How many times this instance was asked for "Move to List…"'s entries. */
let listsAsked = 0;

/** "Delta note": its submenus. */
function delta(): Item {
  return {
    id: "delta",
    title: DELTA,
    subtitle: "Submenus, given at once or asked for when opened",
    actions: [
      action("Open", DELTA),
      {
        title: "Open With…",
        submenu: {
          title: "Open With",
          entries: [
            answering("Notepad", "Open With Notepad", DELTA, {
              section: "Editors",
              shortcut: { modifiers: ["ctrl", "shift"], key: "n" },
            }),
            answering("WordPad", "Open With WordPad", DELTA, { section: "Editors" }),
            answering("Browser", "Open With Browser", DELTA, {
              section: "Other",
              shortcut: { modifiers: ["ctrl", "shift"], key: "b" },
            }),
            action("Forget Applications", DELTA, {
              section: "Danger",
              style: "destructive",
              shortcut: { modifiers: ["ctrl", "shift"], key: "d" },
            }),
          ],
        },
      },
      {
        title: "Move to List…",
        section: "Organize",
        submenu: {
          title: "Move to List",
          async onOpen(): Promise<Action[]> {
            listsAsked += 1;
            const section = `Asked ${listsAsked} ${listsAsked === 1 ? "time" : "times"}`;
            return ["Inbox", "Later", "Someday"].map((list) =>
              answering(list, `Move to ${list}`, DELTA, { section }),
            );
          },
        },
      },
      {
        title: "Tag…",
        section: "Organize",
        submenu: {
          title: "Tags",
          async onOpen(): Promise<Action[]> {
            throw new Error("The tags could not be loaded");
          },
        },
      },
    ],
  };
}

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
        delta(),
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
