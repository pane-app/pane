// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's icons sample in TypeScript (#139): a list whose rows show every
// kind of icon and accessory, with tooltips. Rows, icons, accessories and
// answers match the Rust icons sample (guests/sample-icons) and the
// JavaScript one; see the Rust sample for what each row shows. Its package
// has an icon of its own and its "Icons" command another; its second
// command has none, so it shows the package's. The date accessory is given
// as milliseconds here (as a `Date` in the JavaScript sample).
import type { Accessory, Command, CustomView, Item, List } from "@pane/extension";
import { avatar, progressRing } from "@pane/extension/icons";

/** 2026-01-01T00:00:00Z: the "Packaged image" row's date. */
const NEW_YEAR = Date.UTC(2026, 0, 1);

/** The row `id` titled `title`, whose action answers "Chose <title>". */
function row(id: string, title: string, more: Partial<Item>): Item {
  return { id, title, onAction: async () => `Chose ${title}`, ...more };
}

const crowded: Accessory[] = ["1", "2", "3", "4", "5"].map((text) => ({ text }));

export const command: Command = {
  async render(): Promise<List> {
    return {
      title: "Icons sample",
      items: [
        row("builtin", "Built-in icon", {
          subtitle: "reicon's star, by name",
          icon: { builtin: "star" },
          titleTooltip: "A built-in icon from the whole reicon set",
          accessories: [{ text: "3", tooltip: "Unread" }],
        }),
        row("packaged", "Packaged image", {
          subtitle: "Its @light and @dark variants follow the theme",
          icon: "assets/logo.png",
          subtitleTooltip: "logo@light.png in the light theme, logo@dark.png in the dark",
          accessories: [{ date: NEW_YEAR }],
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
          accessories: crowded,
        }),
      ],
    };
  },

  async submitForm(itemId: string): Promise<string> {
    throw new Error(`The icons sample has no forms: ${itemId}`);
  },

  async openView(itemId: string): Promise<CustomView> {
    throw new Error(`The icons sample has no custom views: ${itemId}`);
  },
};
