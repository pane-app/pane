// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's handoff sample in TypeScript: a command that keeps a counter and
// a draft in memory and opts in to handing them to its new code (ADR 0041,
// #159), so a Reload, an Update or a development-mode reload finds them
// where it left them — and its screen, which Pane opens again either way,
// with the launch record it was opened with. "Add one" counts; "Edit the
// draft" opens the draft's form, whose text it saves. Nothing is written
// to Pane's extension data: without the handoff, a replacement loses both.
// The state is a value this module versions itself, which
// `@pane-app/extension/state` serialises. The draft's form is a designed
// view the item pushes (#241), as the typed form it was is retired. Items,
// titles and answers match the Rust and JavaScript handoff samples.
import type { Command } from "@pane-app/extension";
import type { Lifecycle } from "@pane-app/extension/state";
import type { FormSubmittedValues } from "@pane-app/extension/view";
import { showToast } from "@pane-app/extension/feedback";
import { launch } from "pane:extension/commands@0.2.0";
import { Form, Text, createView, useState } from "@pane-app/extension/view";
import { jsxs } from "@pane-app/extension/jsx-runtime";
import { load, save } from "@pane-app/extension/state";

/**
 * The state this instance keeps in memory: the counter and the draft,
 * handed to the new code on a replacement.
 */
interface Kept {
  counter: number;
  draft: string;
}

/** The state, kept in this instance's memory: lost with it, except what the handoff carries. */
let kept: Kept | undefined;

/** The state as it stands, handing nothing over until there is some. */
function state(): Kept {
  if (kept === undefined) {
    kept = { counter: 0, draft: "" };
  }
  return kept;
}

/** `text` in quotes, or "nothing" for empty. */
function quoted(text: string): string {
  return text === "" ? "nothing" : `“${text}”`;
}

/**
 * The state handoff's entry points, beside the command's own export. This
 * sample declares no activation entry point, so Pane never calls
 * `activate`: exporting the lifecycle interface opts in to the state
 * handoff, and `activate` is here because the interface has it.
 */
export const lifecycle: Lifecycle = {
  async activate() {},

  // The state handed to the new code: the counter and the draft, as the
  // SDK serialises them. Resolving with nothing hands nothing over.
  async snapshot() {
    return save(state());
  },

  // Restores what a replaced instance handed over. Throwing discards the
  // state and starts fresh, which is not a failure: this throws when the
  // bytes are not this version's state.
  async restore(bytes: Uint8Array) {
    kept = load(bytes) as Kept;
  },
};

/** The command's list: the counter and the draft as they stand, with the actions that change them. */
export const command: Command = {
  // The "draft" item's action launches this view: the draft's form, as a
  // designed view the command opens.
  async openView(commandId: string) {
    if (commandId !== "draft") {
      throw new Error(`unknown command: ${commandId}`);
    }
    return createView(DraftView);
  },

  // Stable item ids, so a reopened screen keeps its selection.
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
          onAction: async () => {
            launch({ command: "draft" }, "user-initiated", [], null);
          },
        },
      ],
    };
  },
};

/** The draft's form as a designed view (#241): a form with the draft's
 * field, its submission saving it — the answer drawing over the form,
 * as a form's submission answers. Saving pops the view, which re-renders
 * the list with the draft it kept. */
function DraftView() {
  const [saved, setSaved] = useState("");
  const submit = (values: FormSubmittedValues) => {
    const text = String(values.draft ?? "").trim();
    if (text === "") {
      setSaved("Enter the draft");
      return;
    }
    state().draft = text;
    setSaved(`Saved the draft ${quoted(text)}`);
  };
  return jsxs(Form, {
    key: "form",
    navigationTitle: "Edit the draft",
    submitTitle: "Save",
    onSubmit: submit,
    children: [
      ...(saved === "" ? [] : [jsxs(Text, { style: "title", children: [saved] })]),
      jsxs(Form.TextField, {
        key: "draft",
        id: "draft",
        label: "Draft",
        placeholder: "What a replacement must keep",
        value: state().draft,
        autoFocus: true,
        error: saved === "Enter the draft" ? "Enter the draft" : undefined,
      }),
    ],
  });
}
