// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Icons, accessories and tooltips of a list's items (#139), and the icon
// helpers of `@pane/extension/icons` (icons.js). The shapes are the
// tree's (docs/list-tree.md, "Icons" and "Accessories"), which the
// adapter writes as they are given.

/**
 * A theme tone: Pane's text levels and accent, or a colour each theme
 * draws in its own shade.
 */
export type Tone =
  | "primary"
  | "secondary"
  | "accent"
  | "red"
  | "orange"
  | "yellow"
  | "green"
  | "blue"
  | "purple"
  | "magenta";

/**
 * One colour: a tone, or a raw colour (`#rgb`, `#rgba`, `#rrggbb`,
 * `#rrggbbaa`), which Pane corrects for contrast against what it is drawn
 * on.
 */
export type Color = Tone | `#${string}`;

/** A colour for both themes, or one for the light and one for the dark. */
export type Tint = Color | { light: Color; dark: Color };

/** What every icon may have besides what it draws. */
export interface IconOptions {
  /** A built-in icon's colour, or an image drawn as a mask in it. */
  tint?: Tint | null;
  /** The shape the icon is clipped to. */
  mask?: "circle" | "rounded-rectangle" | null;
  /**
   * Drawn when this icon cannot be: an unknown name, a missing image, a
   * web image while it loads or if it fails, a path that does not exist.
   */
  fallback?: Icon | null;
  /**
   * Shown on hover and read by assistive technology; an icon without one
   * is decoration.
   */
  tooltip?: string | null;
}

/**
 * An icon:
 *
 * - a built-in icon by name, reicon's in kebab case (`"star"`,
 *   `"arrow-up-right"`), or `{ builtin, filled? }`;
 * - a PNG or SVG image the package ships, by its path (`"assets/logo.png"`;
 *   `logo@dark.png` and `logo@light.png` beside it are drawn in the dark
 *   and light themes), or `{ path }`, or a pair `{ light, dark }`;
 * - an image by URL, `{ url }`: a `data:` URL, or a web image by `http(s)`
 *   address, which Pane downloads and keeps as the extension's cache, the
 *   list showing the fallback (or a neutral placeholder) until it arrives
 *   and if it fails;
 * - the system's icon of a file, folder or application by its path,
 *   `{ file }` (absolute, or from `~/`), drawn bare;
 *
 * each with any of the options.
 */
export type Icon = string | IconObject;

/** An icon written as an object (see [`Icon`]). */
export type IconObject = IconOptions &
  (
    | { builtin: string; filled?: boolean | null }
    | { path: string }
    | { light: string; dark: string }
    | { url: string }
    | { file: string }
  );

/** What every accessory may have besides what it shows. */
export interface AccessoryOptions {
  /** Drawn before its text; an accessory may be an icon alone. */
  icon?: Icon | null;
  /** The colour of its text, or of a tag. */
  color?: Tint | null;
  /** Shown on hover; a date's is its absolute time unless it has one. */
  tooltip?: string | null;
}

/**
 * One accessory on the right of a row: text (`{ text }`), a time shown
 * relative to now and kept current (`{ date }`, a `Date` or milliseconds
 * since the Unix epoch), or a coloured tag (`{ tag }`), or an icon alone.
 * A row shows at most three.
 */
export type Accessory = AccessoryOptions &
  ({ text: string } | { date: Date | number } | { tag: string } | { icon: Icon });

/**
 * An avatar of `name`'s initials: up to two letters, white on a circle
 * whose colour `name` picks.
 */
export function avatar(name: string): IconObject;

/**
 * A progress ring `fraction` full (0 to 1), drawn in the accent; give it
 * another `tint` to draw it in another colour.
 */
export function progressRing(fraction: number): IconObject;

/**
 * The favicon of the website `url` is on: its `/favicon.ico`, a web image
 * Pane downloads, with a globe in the secondary tone as its fallback.
 */
export function favicon(url: string): IconObject;

/**
 * The system's icon of the file, folder or application at `path`, with a
 * document in the secondary tone as its fallback.
 */
export function fileIcon(path: string): IconObject;
