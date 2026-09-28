// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Types for Pane's extension contract, `pane:extension/command` in
// wit/extension.wit, as JavaScript and TypeScript commands see it. They
// describe plain values only; nothing here is specific to the JS engine.
/// <reference path="./wasi.d.ts" />

/** One entry in a command's list view. */
export interface Item {
  /** Passed back to `runAction` when the user activates the item. */
  id: string;
  title: string;
  /** A second line under the title; omitted or `null` for none. */
  subtitle?: string | null;
}

/** A command's list view. */
export interface View {
  title: string;
  items: Item[];
}

/**
 * An extension command. The module exports it as `command`:
 *
 * ```ts
 * export const command: Command = { async getView() { ... }, async runAction(id) { ... } };
 * ```
 *
 * Resolving gives Pane the value. Throwing (rejecting) reports an error to the
 * user: an `Error`'s message, or a thrown string as is. Resolving with a value
 * of the wrong type, such as `undefined` instead of a string, is a crash: Pane
 * reports it and starts a fresh instance for the next call.
 */
export interface Command {
  /** Produce the command's list view. */
  getView(): Promise<View>;
  /** Run the action of the item with `itemId`; the text is shown as the result. */
  runAction(itemId: string): Promise<string>;
}
