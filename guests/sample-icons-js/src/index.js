// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's icons sample in JavaScript (#139): a list whose rows show every
// kind of icon and accessory, with tooltips. Rows, icons, accessories and
// answers match the Rust icons sample (guests/sample-icons) and the
// TypeScript one; see the Rust sample for what each row shows. Its package
// has an icon of its own and its "Icons" command another; its second
// command has none, so it shows the package's. The date accessory is given
// as a `Date` here (as milliseconds in the TypeScript sample), which the
// adapter writes as milliseconds.
// @ts-check
import { avatar, progressRing } from "@pane/extension/icons";

/**
 * The row `id` titled `title`, whose action answers "Chose <title>".
 * @param {string} id
 * @param {string} title
 * @param {Partial<import("@pane/extension").Item>} more
 * @returns {import("@pane/extension").Item}
 */
function row(id, title, more) {
  return { id, title, onAction: async () => `Chose ${title}`, ...more };
}

/** @type {import("@pane/extension").Command} */
export const command = {
  async render() {
    return {
      title: "Icons sample",
      items: [
        row("builtin", "Built-in icon", {
          subtitle: "reicon's star, by name",
          icon: "star",
          titleTooltip: "A built-in icon from the whole reicon set",
          accessories: [{ text: "3", tooltip: "Unread" }],
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
