// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's icons sample in JavaScript (#139): a list whose rows show every
// kind of icon and accessory, with tooltips. Rows, icons, accessories and
// answers match the Rust icons sample (guests/sample-icons) and the
// TypeScript one; see the Rust sample for what each row shows. Its package
// has an icon of its own and its "Icons" command another; its second
// command has none, so it shows the package's. The date accessory is given
// as a `Date` here (as milliseconds in the TypeScript sample), which the
// adapter writes as milliseconds. The web images come from the server the
// `imageServer` setting names, and the system icons are of the file and
// the application the `iconFile` and `iconApplication` settings name, as
// in the Rust sample (#142). "Built-in icon" has two more actions with
// icons in the Actions panel, "Copy Name" and "Open Image", and every row's
// action and each of these tells the user "Chose <title>" in a toast.
// @ts-check
import { showToast } from "@pane/extension/feedback";
import { avatar, favicon, fileIcon, progressRing } from "@pane/extension/icons";
import { get } from "pane:extension/settings@0.1.0";

/** The setting naming the server the web images come from. */
const IMAGE_SERVER = "imageServer";
/** The setting naming the file whose icon "File icon" shows. */
const ICON_FILE = "iconFile";
/** The setting naming the application whose icon "Application icon" shows. */
const ICON_APPLICATION = "iconApplication";

/**
 * The value of the setting `key`: the saved one, or `fallback`.
 * @param {string} key
 * @param {string} fallback
 * @returns {string}
 */
function setting(key, fallback) {
  try {
    const value = get(key);
    return value != null && value.trim() !== "" ? value : fallback;
  } catch {
    return fallback;
  }
}

/**
 * Tells the user they chose `title`, in a toast: "Chose <title>".
 * @param {string} title
 * @returns {Promise<void>}
 */
async function chose(title) {
  showToast({ title: `Chose ${title}` });
}

/**
 * The row `id` titled `title`, whose action tells the user "Chose <title>".
 * @param {string} id
 * @param {string} title
 * @param {Partial<import("@pane/extension").Item>} more
 * @returns {import("@pane/extension").Item}
 */
function row(id, title, more) {
  return { id, title, onAction: () => chose(title), ...more };
}

/**
 * The action titled `title` with `icon` beside it in the Actions panel,
 * which tells the user "Chose <title>".
 * @param {string} title
 * @param {import("@pane/extension").Icon} icon
 * @returns {import("@pane/extension").Action}
 */
function action(title, icon) {
  return { title, icon, onAction: () => chose(title) };
}

/** @type {import("@pane/extension").Command} */
export const command = {
  async render() {
    const server = setting(IMAGE_SERVER, "http://127.0.0.1:8741").replace(/\/+$/, "");
    const slow = `${server}/images/slow.png`;
    return {
      title: "Icons sample",
      items: [
        row("builtin", "Built-in icon", {
          subtitle: "reicon's star, by name",
          icon: "star",
          titleTooltip: "A built-in icon from the whole reicon set",
          accessories: [{ text: "3", tooltip: "Unread" }],
          actions: [
            action("Copy Name", "copy"),
            action("Open Image", { url: slow, fallback: { builtin: "clock", tint: "secondary" } }),
          ],
        }),
        row("packaged", "Packaged image", {
          subtitle: "Its @light and @dark variants follow the theme",
          icon: { path: "assets/logo.png" },
          subtitleTooltip: "logo@light.png in the light theme, logo@dark.png in the dark",
          accessories: [{ date: new Date(Date.UTC(2026, 0, 1)) }],
        }),
        row("pair", "Light and dark pair", {
          subtitle: "A sun in the light theme, a moon in the dark",
          icon: { light: "assets/sun.svg", dark: "assets/moon.svg" },
          accessories: [{ tag: "Open", color: "green" }],
        }),
        row("tinted", "Tinted icon", {
          subtitle: "reicon's heart in a raw colour",
          icon: { builtin: "heart", tint: "#ff6363" },
          accessories: [{ text: "tinted", color: { light: "#b42318", dark: "#ff8a80" } }],
        }),
        row("masked", "Masked image", {
          subtitle: "A photo clipped to a circle",
          icon: { path: "assets/photo.png", mask: "circle" },
          accessories: [{ icon: { builtin: "user", tint: "blue", tooltip: "Owner" } }],
        }),
        row("fallback", "Failing image", {
          subtitle: "Its image is missing, so its fallback shows",
          icon: {
            path: "assets/missing.png",
            fallback: { builtin: "warning", tint: "orange" },
            tooltip: "Image missing",
          },
        }),
        row("avatar", "Avatar and progress", {
          subtitle: "Built by the SDK's helpers",
          icon: avatar("Ada Lovelace"),
          accessories: [{ text: "40%", icon: progressRing(0.4) }],
        }),
        row("crowded", "Crowded row", {
          subtitle: "Five accessories, of which a row draws three",
          accessories: ["1", "2", "3", "4", "5"].map((text) => ({ text })),
        }),
        row("favicon", "Favicon", {
          subtitle: "The site's favicon, downloaded and cached by Pane",
          icon: favicon(`${server}/`),
        }),
        row("slow", "Slow web image", {
          subtitle: "Its fallback shows until the image arrives",
          icon: { url: slow, fallback: { builtin: "clock", tint: "secondary" } },
        }),
        row("same", "Same slow image", {
          subtitle: "The same address, downloaded once",
          icon: { url: slow, fallback: { builtin: "clock", tint: "secondary" } },
        }),
        row("broken", "Broken image", {
          subtitle: "Its address has no image, so its fallback stays",
          icon: {
            url: `${server}/images/missing.png`,
            fallback: { builtin: "link-broken", tint: "orange" },
            tooltip: "Image unavailable",
          },
        }),
        row("file", "File icon", {
          subtitle: "The system's icon of a file",
          icon: fileIcon(setting(ICON_FILE, "~")),
        }),
        row("application", "Application icon", {
          subtitle: "The system's icon of an application, drawn bare",
          icon: fileIcon(setting(ICON_APPLICATION, "C:\\Windows\\System32\\notepad.exe")),
        }),
      ],
    };
  },

  async submitForm(itemId) {
    throw new Error(`The icons sample has no forms: ${itemId}`);
  },

  async openView(itemId) {
    throw new Error(`The icons sample has no custom views: ${itemId}`);
  },
};
