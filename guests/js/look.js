// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// How an item looks beyond its title and subtitle (#139), as the adapter
// (adapt.js) writes it into the tree (docs/list-tree.md, "Icons" and
// "Accessories"): its icon, its title's and subtitle's tooltips and its
// accessories. Icons are written as the command gives them (a name, an
// image's path or an object, the same shapes the tree has); an accessory's
// `date`, a `Date` or milliseconds since the Unix epoch, becomes
// milliseconds. What Pane cannot read of an icon or an accessory is left
// out by Pane, never a crash.

/** `date` as milliseconds since the Unix epoch, or `undefined`. */
function millis(date) {
  if (date instanceof Date) return date.getTime();
  if (typeof date === "number") return date;
  return undefined;
}

/** `accessory` as the tree writes it. */
function accessoryNode(accessory) {
  if (accessory === null || typeof accessory !== "object") return accessory;
  const node = { ...accessory };
  if ("date" in node) node.date = millis(node.date);
  return node;
}

/**
 * The fields `item` adds to its tree node for how it looks: `icon`,
 * `titleTooltip`, `subtitleTooltip` and `accessories`, those it has.
 */
export function look(item) {
  const node = {};
  if (item?.icon != null) node.icon = item.icon;
  if (item?.titleTooltip != null) node.titleTooltip = item.titleTooltip;
  if (item?.subtitleTooltip != null) node.subtitleTooltip = item.subtitleTooltip;
  if (Array.isArray(item?.accessories)) node.accessories = item.accessories.map(accessoryNode);
  return node;
}
