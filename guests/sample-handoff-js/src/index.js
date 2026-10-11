// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's handoff sample in JavaScript: a command that keeps a counter and
// a draft in memory and opts in to handing them to its new code (ADR
// 0041, #159), so a Reload, an Update or a development-mode reload finds
// them where it left them — and its screen, which Pane opens again either
// way, with the launch record it was opened with. "Add one" counts; "Edit
// the draft" opens the draft's form, whose text it saves. Nothing is
// written to Pane's extension data: without the handoff, a replacement
// loses both. The state is a value this module versions itself, which
// `@pane-app/extension/state` serialises. Items, titles and answers match
// the Rust and TypeScript handoff samples.
// @ts-check
import { showToast } from "@pane-app/extension/feedback";
import { save, load } from "@pane-app/extension/state";

/**
 * The state this instance keeps in memory: the counter and the draft,
 * handed to the new code on a replacement.
 * @typedef {object} Kept
 * @property {number} counter
 * @property {string} draft
 */

/** @type {Kept | undefined} */
let kept;

/**
 * The state as it stands, handing nothing over until there is some.
 * @returns {Kept}
 */
function state() {
  if (kept === undefined) {
    kept = { counter: 0, draft: "" };
  }
  return kept;
}

/**
 * The draft's form: one text field.
 * @type {import("@pane-app/extension").Form}
 */
const DRAFT_FORM = {
  title: "Edit the draft",
  fields: [
    {
      id: "draft",
      label: "Draft",
      kind: { tag: "text", val: { placeholder: "What a replacement must keep" } },
    },
  ],
  submitLabel: "Save",
};

/**
 * `text` in quotes, or "nothing" for empty.
 * @param {string} text
 * @returns {string}
 */
function quoted(text) {
  return text === "" ? "nothing" : `“${text}”`;
}

/**
 * The state handoff's entry points, beside the command's own export.
 * @type {import("@pane-app/extension/state").Lifecycle}
 */
export const lifecycle = {
  // This sample declares no activation entry point, so Pane never calls
  // this: exporting the lifecycle interface opts in to the state
  // handoff, and `activate` is here because the interface has it.
  async activate() {},

  // The state handed to the new code: the counter and the draft, as the
  // SDK serialises them. Resolving with nothing hands nothing over.
  async snapshot() {
    return save(state());
  },

  /**
   * Restores what a replaced instance handed over. Throwing discards the
   * state and starts fresh, which is not a failure: this throws when the
   * bytes are not this version's state.
   * @param {Uint8Array} bytes
   */
  async restore(bytes) {
    kept = /** @type {Kept} */ (load(bytes));
  },
};

/** @type {import("@pane-app/extension").Command} */
export const command = {
  // The command's list: the counter and the draft as they stand, with
  // the actions that change them. Stable item ids, so a reopened screen
  // keeps its selection.
  async render() {
    const { counter, draft } = state();
    return {
      title: `Handoff: ${counter} counted, draft ${quoted(draft)}`,
      items: [
        {
          id: "add",
          title: `Add one (${counter} so far)`,
          onAction: async () => {
            state().counter += 1;
            showToast({ title: "Counted one more" });
          },
        },
        {
          id: "draft",
          title: `Edit the draft (${draft.split(/\s+/).filter(Boolean).length} words)`,
          subtitle: `The draft is ${quoted(draft)}`,
          form: DRAFT_FORM,
        },
      ],
    };
  },

  // The draft form's answer: the draft becomes the text typed, and the
  // screen says so.
  async submitForm(itemId, values) {
    if (itemId !== "draft") {
      throw { message: `unknown form: ${itemId}` };
    }
    const text = values.find((field) => field.id === "draft")?.value ?? "";
    state().draft = text;
    return `Saved the draft ${quoted(text)}`;
  },
};
