// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `@pane/extension/preferences` (preferences.js): the
// preferences the command's package declares in `pane.json`, as the user
// set them in Pane.

/**
 * The effective preference values of the command Pane is running, or of
 * the command with id `command` of the same package: each declared name to
 * the value the user set or else its default. A checkbox's value is a
 * boolean, every other kind's a string (a dropdown's, the chosen option's
 * `value`; a file's, folder's or application's, its path); a preference
 * with no value and no default is absent. Declare the shape as an
 * interface and name it: `getPreferenceValues<Preferences>()`. Throws an
 * `Error` with Pane's reason when it refuses.
 */
export function getPreferenceValues<T = Record<string, string | boolean>>(command?: string): T;
