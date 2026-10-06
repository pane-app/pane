// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's actions sample in TypeScript: a list whose items carry several
// actions (#137), and what a command does after it acts (#141). Items,
// actions, sections, shortcuts, toasts and commands match the Rust actions
// sample (guests/sample-actions) and the JavaScript one.
// "Alpha note" has nine actions, in an untitled section and the "Edit",
// "Share" and "Danger" sections: Enter runs "Open", Ctrl+Enter "Copy" and
// Ctrl+Shift+Enter "Rename", and Ctrl+K lists them all. Its shortcuts show
// the rules Pane binds them by: one per system ("Reveal"), one that is
// Pane's own Ctrl+K and so is never bound ("Open Menu"), one that Pane
// leaves free until the user gives one of its keys Ctrl+Shift+Y
// ("Archive"), and a destructive "Delete". "Beta note" has one action, so
// Ctrl+Enter does nothing there, and "Gamma note" has none, so it cannot be
// activated. Every one of their actions shows a success toast with its
// title and the item's ("Open: Alpha note").
// "Window" closes the window with each way the next showing may go, pops to
// root search and clears the search field; "In the Background" launches
// the "Window functions" no-view command in the background, whose toast
// says what each function answered there. "Feedback" shows a HUD (and a
// failure HUD), a toast updated from animated ("Uploading…") to success
// ("Uploaded") with Open and Retry actions, hides it, fails, and sets and
// clears the command's row subtitle ("3 unread"). The no-view commands
// "Spin" (it leaves an animated toast, which Pane hides when the run ends)
// and "Stumble" (it fails) show what Pane does at a run's end.
//
// "Delta note" shows submenus (#140): "Open With…" gives its entries at once,
// in sections, two with shortcuts and a destructive one; "Move to List…"
// gives them when it opens, in a section that counts how many times it was
// asked ("Asked 1 time"); and "Tag…" fails when it opens ("The tags could
// not be loaded"). An entry shows what it did and the item's ("Open With
// Notepad: Delta note", "Move to Later: Delta note").
//
// "System" calls the system host functions (#145) one by one, each doing
// only what it names and then saying so in a toast ("Copy Text: done"):
// plain and concealed copies of text and of a file, a clipboard read
// ("Clipboard: text “…”"), opens of an `https:`, a `mailto:` and an
// `ms-settings:` link, a file, a folder and an application, an open with a
// named application, a reveal and a trash of two files ("Delete me.txt" and
// "Keep me.txt" in a `pane-sample` folder, which do not exist unless the
// user makes them). The paths are this system's (`places`). "Standard
// actions" has every standard action of `@pane/extension/system`: Copy
// (Enter), a concealed Copy, a Copy that keeps the window open, a Copy of a
// file, Open, Open With… (the installed applications), Show in Explorer and
// Move to Recycle Bin; each closes the window after it acts.
//
// "Confirm" asks before it acts (#146): its destructive "Delete" asks
// "Delete the note?" offering "Don't ask again" (remembered under
// `delete-note`) and toasts "Deleted" or "Kept"; "Ask" asks "Go on?" with its
// own buttons and nothing to remember ("Went on" or "Stopped"); "Close and
// Ask" closes the window, then asks, so Pane shows itself again for it;
// "Ask in the Background" launches the no-view "Confirm Run" in the
// background, where it toasts "Not asked" with the reason. Run by the user,
// "Confirm Run" asks "Run it?" and toasts "Ran" or "Did not run".
import type { Action, Command, CustomView, Item, List, Shortcut } from "@pane/extension";
import {
  clearSearchBar,
  closeMainWindow,
  confirmAlert,
  popToRoot,
  setSubtitle,
  showHUD,
  showToast,
  type Toast,
  type ToastOptions,
} from "@pane/extension/feedback";
import { launch } from "pane:extension/commands@0.1.0";
import {
  copy,
  copyAction,
  moveToTrashAction,
  open,
  openAction,
  openWithAction,
  readClipboard,
  runningOn,
  showInFileManager,
  showInFileManagerAction,
  trash,
} from "@pane/extension/system";

/** What an action may say besides its title. */
type More = { section?: string; style?: "destructive"; shortcut?: Shortcut };

/** Toasts `yes` or `no`, as the user answered. */
function said(answer: boolean, yes: string, no: string): void {
  showToast({ title: answer ? yes : no });
}

/** The action titled `title` of the item titled `item`: it shows both. */
function action(title: string, item: string, more: More = {}): Action {
  return answering(title, title, item, more);
}

/**
 * The action titled `title` of the item titled `item` that shows `said` and
 * the item's title.
 */
function answering(title: string, said: string, item: string, more: More = {}): Action {
  return {
    title,
    onAction: async () => {
      showToast({ title: `${said}: ${item}` });
    },
    ...more,
  };
}

/**
 * What a window function answered: nothing more when a window was shown for
 * the call, else the failure that none was.
 */
function windowed(shown: boolean): void {
  if (!shown) throw new Error("No window was shown");
}

/** The "Window" item's action titled `title`, which runs `run`. */
function windowAction(title: string, run: () => boolean): Action {
  return {
    title,
    onAction: async () => {
      windowed(run());
    },
  };
}

/**
 * The upload toast "Start Upload" showed, which "Finish Upload" and "Hide
 * Toast" change.
 */
let upload: Toast | null = null;

/** Starts the upload: an animated toast, remembered. */
function startUpload(): Toast {
  const shown = showToast({ style: "animated", title: "Uploading…" });
  upload = shown;
  return shown;
}

/** The upload's toast once it is done: a success with Open and Retry. */
function uploaded(): ToastOptions {
  return {
    style: "success",
    title: "Uploaded",
    message: "notes.txt",
    primaryAction: {
      title: "Open",
      onAction: async () => {
        showToast({ title: "Opened the upload" });
      },
      shortcut: { modifiers: ["ctrl", "shift"], key: "o" },
    },
    secondaryAction: {
      title: "Retry",
      onAction: async () => {
        startUpload().update(uploaded());
      },
      shortcut: { modifiers: ["ctrl", "shift"], key: "r" },
    },
  };
}

/** The text the sample copies. */
const COPIED_TEXT = "Copied by the actions sample";

/** The secret the sample copies concealed. */
const SECRET = "hunter2";

/**
 * Where the sample's opens, copies, reveals and trashes point on this
 * system: a file, a folder and an application every such system has, and
 * two files to trash that do not exist unless the user makes them.
 */
interface Places {
  file: string;
  folder: string;
  application: string;
  trash: [string, string];
}

function places(): Places {
  switch (runningOn()) {
    case "windows":
      return {
        file: "C:\\Windows\\win.ini",
        folder: "C:\\Windows",
        application: "C:\\Windows\\System32\\notepad.exe",
        trash: ["C:\\pane-sample\\Delete me.txt", "C:\\pane-sample\\Keep me.txt"],
      };
    case "macos":
      return {
        file: "/etc/hosts",
        folder: "/Applications",
        application: "/System/Applications/TextEdit.app",
        trash: ["/tmp/pane-sample/Delete me.txt", "/tmp/pane-sample/Keep me.txt"],
      };
    default:
      return {
        file: "/etc/hosts",
        folder: "/tmp",
        application: "/usr/bin/xdg-open",
        trash: ["/tmp/pane-sample/Delete me.txt", "/tmp/pane-sample/Keep me.txt"],
      };
  }
}

/**
 * The "System" item's action titled `title`, which runs `run` and then says
 * it is done in a toast.
 */
function systemAction(title: string, run: () => void): Action {
  return {
    title,
    onAction: async () => {
      run();
      showToast({ title: `${title}: done` });
    },
  };
}

/** "System": each host function on its own. */
function systemItem(): Item {
  return {
    id: "system",
    title: "System",
    subtitle: "The clipboard, opening, revealing and recycling, one by one",
    actions: [
      systemAction("Copy Text", () => copy(COPIED_TEXT)),
      systemAction("Copy Text Concealed", () => copy(SECRET, { concealed: true })),
      systemAction("Copy File", () => copy({ file: places().file })),
      systemAction("Copy File Concealed", () => copy({ file: places().file }, { concealed: true })),
      {
        title: "Read Clipboard",
        onAction: async () => {
          const clip = readClipboard();
          const said =
            clip == null
              ? "Clipboard: empty"
              : clip.file != null
                ? `Clipboard: file ${clip.file}`
                : `Clipboard: text “${clip.text}”`;
          showToast({ title: said });
        },
      },
      systemAction("Open Website", () => open("https://example.com")),
      systemAction("Open Mail", () => open("mailto:someone@example.com")),
      systemAction("Open Settings", () => open("ms-settings:display")),
      systemAction("Open File", () => open(places().file)),
      systemAction("Open Folder", () => open(places().folder)),
      systemAction("Open Application", () => open(places().application)),
      systemAction("Open File With Application", () => open(places().file, places().application)),
      systemAction("Reveal File", () => showInFileManager(places().file)),
      systemAction("Trash Files", () => trash(places().trash)),
    ],
  };
}

/**
 * "Standard actions": every standard action, each closing the window after
 * it acts but the one that keeps it open.
 */
function standardItem(): Item {
  const {
    file,
    trash: [deleteMe],
  } = places();
  return {
    id: "standard",
    title: "Standard actions",
    subtitle: "Copy, Open, Open With…, Show in Explorer, Move to Recycle Bin",
    actions: [
      copyAction(COPIED_TEXT),
      copyAction(SECRET, { concealed: true, title: "Copy Password" }),
      copyAction(COPIED_TEXT, { keepWindowOpen: true, title: "Copy and Keep Open" }),
      copyAction({ file }, { title: "Copy File" }),
      openAction("https://example.com"),
      openWithAction(file),
      showInFileManagerAction(file),
      moveToTrashAction([deleteMe]),
    ],
  };
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
        {
          id: "window",
          title: "Window",
          subtitle: "Close, pop to root search, clear the search",
          actions: [
            windowAction("Close", () => closeMainWindow()),
            windowAction("Close to Root Search", () =>
              closeMainWindow({ popToRootType: "immediate" }),
            ),
            windowAction("Close and Keep Screen", () =>
              closeMainWindow({ popToRootType: "suspended" }),
            ),
            windowAction("Close and Clear Root Search", () =>
              closeMainWindow({ clearRootSearch: true }),
            ),
            windowAction("Pop to Root", () => popToRoot()),
            windowAction("Pop to Root and Clear Search", () => popToRoot({ clearSearchBar: true })),
            windowAction("Clear Search", () => clearSearchBar()),
            {
              title: "In the Background",
              onAction: async () => {
                launch({ command: "window-functions" }, "background", [], null);
              },
            },
          ],
        },
        {
          id: "feedback",
          title: "Feedback",
          subtitle: "A HUD, toasts and this command's subtitle",
          actions: [
            {
              title: "Show HUD",
              onAction: async () => {
                showHUD("Copied to Clipboard");
              },
            },
            {
              title: "Show Failure HUD",
              onAction: async () => {
                showHUD("Could not copy", "failure");
              },
            },
            {
              title: "Start Upload",
              onAction: async () => {
                startUpload();
              },
            },
            {
              title: "Finish Upload",
              onAction: async () => {
                if (upload === null) throw new Error("Nothing is uploading");
                upload.update(uploaded());
              },
            },
            {
              title: "Upload",
              onAction: async () => {
                startUpload().update(uploaded());
              },
            },
            {
              title: "Hide Toast",
              onAction: async () => {
                if (upload !== null) upload.hide();
                upload = null;
              },
            },
            {
              title: "Fail",
              onAction: async () => {
                throw new Error("The upload failed");
              },
            },
            {
              title: "Set Subtitle",
              onAction: async () => {
                setSubtitle("3 unread");
              },
            },
            {
              title: "Clear Subtitle",
              onAction: async () => {
                setSubtitle(null);
              },
            },
          ],
        },
        {
          id: "confirm",
          title: "Confirm",
          subtitle: "Asks before it acts, remembering the answer or not",
          actions: [
            {
              title: "Delete",
              style: "destructive",
              onAction: async () => {
                const answer = await confirmAlert({
                  title: "Delete the note?",
                  message: "It cannot be brought back.",
                  primaryAction: { title: "Delete", style: "destructive" },
                  remember: "delete-note",
                });
                said(answer, "Deleted", "Kept");
              },
            },
            {
              title: "Ask",
              onAction: async () => {
                const answer = await confirmAlert({
                  title: "Go on?",
                  primaryAction: { title: "Go On" },
                  dismissAction: { title: "Stop" },
                });
                said(answer, "Went on", "Stopped");
              },
            },
            {
              title: "Close and Ask",
              onAction: async () => {
                closeMainWindow();
                const answer = await confirmAlert({
                  title: "Asked while hidden",
                  primaryAction: { title: "Yes" },
                });
                said(answer, "Confirmed while hidden", "Not confirmed while hidden");
              },
            },
            {
              title: "Ask in the Background",
              onAction: async () => {
                launch({ command: "confirm-run" }, "background", [], null);
              },
            },
          ],
        },
        systemItem(),
        standardItem(),
      ],
    };
  },

  async run(id: string): Promise<void> {
    switch (id) {
      // What each window function answers where it runs: in the
      // background, that no window was shown. Launched from root search
      // with its query typed, the close empties that query.
      case "window-functions": {
        const closed = closeMainWindow({ clearRootSearch: true });
        const popped = popToRoot();
        const cleared = clearSearchBar();
        showToast({
          title: `close: ${closed}, pop to root: ${popped}, clear search: ${cleared}`,
        });
        return;
      }
      // Leaves its toast in progress: Pane hides it once the run ends.
      case "spin":
        showToast({ style: "animated", title: "Spinning…" });
        return;
      case "stumble":
        throw new Error("Stumbled on purpose");
      // Asks first; in the background, where Pane asks nothing, says why.
      case "confirm-run": {
        try {
          const answer = await confirmAlert({ title: "Run it?", primaryAction: { title: "Run" } });
          said(answer, "Ran", "Did not run");
        } catch (error) {
          const reason = error instanceof Error ? error.message : String(error);
          showToast({ style: "failure", title: "Not asked", message: reason });
        }
        return;
      }
      default:
        throw new Error(`\`${id}\` opens a screen`);
    }
  },

  async submitForm(itemId: string): Promise<string> {
    // A rejected submitForm is a message about the whole form.
    throw new Error(`The actions sample has no forms: ${itemId}`);
  },

  async openView(itemId: string): Promise<CustomView> {
    throw new Error(`The actions sample has no custom views: ${itemId}`);
  },
};
