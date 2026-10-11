// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// The state handoff's serialization helpers (`@pane-app/extension/state`,
// ADR 0041): a command that opts in hands its in-memory state to the code
// that replaces it — on Reload, an Update and a development-mode reload —
// and restores it on its first start. `save` writes the value `snapshot`
// answers and `load` reads what `restore` receives, as JSON in UTF-8
// bytes. Bundled into the command that imports it, like any npm module.
//
// The bytes are the author's to version: JSON here, so a `restore` that
// cannot read what an older release wrote should throw, which discards the
// state and starts the extension fresh. Pane bounds the bytes (1 MiB at
// most, answered within 1 second) and never writes them anywhere.

export { save, load };

/**
 * `value` as the bytes `lifecycle.snapshot` answers: JSON as UTF-8.
 * @param {unknown} value
 * @returns {Uint8Array}
 */
function save(value) {
  return encoded(JSON.stringify(value));
}

/**
 * The bytes `lifecycle.restore` receives, as the value `save` wrote for
 * it: JSON parsed. Throws when the bytes are not what this release wrote,
 * which discards the state and starts the extension fresh.
 * @param {Uint8Array} state
 */
function load(state) {
  return JSON.parse(decoded(state));
}

/** `text` as its UTF-8 bytes. */
function encoded(text) {
  const bytes = [];
  for (const character of text) {
    const code = character.codePointAt(0);
    if (code < 0x80) {
      bytes.push(code);
    } else if (code < 0x800) {
      bytes.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
    } else if (code < 0x10000) {
      bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
    } else {
      bytes.push(
        0xf0 | (code >> 18),
        0x80 | ((code >> 12) & 0x3f),
        0x80 | ((code >> 6) & 0x3f),
        0x80 | (code & 0x3f),
      );
    }
  }
  return new Uint8Array(bytes);
}

/** `state`'s bytes as the UTF-8 text they hold. */
function decoded(state) {
  let text = "";
  for (let index = 0; index < state.length; ) {
    const first = state[index];
    let code;
    let length;
    if (first < 0x80) {
      code = first;
      length = 1;
    } else if (first < 0xe0) {
      code = ((first & 0x1f) << 6) | (state[index + 1] & 0x3f);
      length = 2;
    } else if (first < 0xf0) {
      code = ((first & 0x0f) << 12) | ((state[index + 1] & 0x3f) << 6) | (state[index + 2] & 0x3f);
      length = 3;
    } else {
      code =
        ((first & 0x07) << 18) |
        ((state[index + 1] & 0x3f) << 12) |
        ((state[index + 2] & 0x3f) << 6) |
        (state[index + 3] & 0x3f);
      length = 4;
    }
    text += String.fromCodePoint(code);
    index += length;
  }
  return text;
}
