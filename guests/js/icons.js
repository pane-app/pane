// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Icon helpers for JS/TS commands (`@pane/extension/icons`, #139): an
// avatar of initials and a progress ring, built from the icons Pane draws
// (an SVG image by `data:` URL, a mask, a tint). Bundled into the command
// that imports it, like any npm module. The Rust SDK's `pane_guest::icon`
// has the same helpers, drawing the same.

/** The colours `avatar` picks from, by name. */
const AVATAR_COLORS = [
  "#e5484d",
  "#f76b15",
  "#ffb224",
  "#30a46c",
  "#12a594",
  "#0090ff",
  "#6e56cf",
  "#d6409f",
];

/** `svg` as a `data:` URL, percent-encoded. */
function svgUrl(svg) {
  return "data:image/svg+xml," + encodeURIComponent(svg);
}

/** `text`'s UTF-8 bytes. */
function utf8(text) {
  const bytes = [];
  for (const c of text) {
    let encoded;
    try {
      encoded = encodeURIComponent(c);
    } catch {
      // A lone surrogate: as U+FFFD.
      encoded = "%EF%BF%BD";
    }
    if (encoded.startsWith("%")) {
      for (const hex of encoded.slice(1).split("%")) bytes.push(parseInt(hex, 16));
    } else {
      bytes.push(encoded.charCodeAt(0));
    }
  }
  return bytes;
}

/** `text` with the characters XML gives a meaning escaped. */
function escapeXml(text) {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

/** FNV-1a of `text`'s UTF-8 bytes: the same name, the same colour. */
function fnv(text) {
  let hash = 0x811c9dc5;
  for (const byte of utf8(text)) {
    hash = Math.imul(hash ^ byte, 0x01000193) >>> 0;
  }
  return hash;
}

/**
 * An avatar of `name`'s initials: up to two letters, white on a circle
 * whose colour `name` picks, as an SVG image clipped to a circle.
 */
export function avatar(name) {
  const initials =
    name
      .split(/\s+/)
      .map((word) => [...word].find((c) => /[\p{L}\p{N}]/u.test(c)))
      .filter((c) => c !== undefined)
      .slice(0, 2)
      .join("")
      .toUpperCase() || "?";
  const color = AVATAR_COLORS[fnv(name) % AVATAR_COLORS.length];
  const size = [...initials].length > 1 ? 24 : 30;
  const svg =
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">' +
    `<circle cx="32" cy="32" r="32" fill="${color}"/>` +
    '<text x="32" y="32" dominant-baseline="central" text-anchor="middle" ' +
    `font-family="sans-serif" font-size="${size}" font-weight="600" ` +
    `fill="#ffffff">${escapeXml(initials)}</text></svg>`;
  return { url: svgUrl(svg), mask: "circle", tooltip: name };
}

/**
 * A progress ring `fraction` full (0 to 1), as an SVG image drawn in the
 * accent; give it another `tint` to draw it in another colour.
 */
export function progressRing(fraction) {
  const clamped = Number.isNaN(fraction) ? 0 : Math.min(1, Math.max(0, fraction));
  // The ring's circumference: 2π × 9.
  const circumference = 56.54867;
  const filled = clamped * circumference;
  const svg =
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">' +
    '<circle cx="12" cy="12" r="9" fill="none" stroke="#000" ' +
    'stroke-opacity="0.25" stroke-width="3"/>' +
    '<circle cx="12" cy="12" r="9" fill="none" stroke="#000" stroke-width="3" ' +
    `stroke-dasharray="${filled.toFixed(2)} ${circumference.toFixed(2)}" ` +
    'transform="rotate(-90 12 12)"/></svg>';
  return { url: svgUrl(svg), tint: "accent" };
}
