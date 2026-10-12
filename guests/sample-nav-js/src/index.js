// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's navigation sample in JavaScript: a designed view with a
// navigation stack (#239) — a list-like view whose rows push a detail
// view, which itself pushes deeper; the detail pops with a result the rows
// answer through `onPop`, and the deeper view drops the whole stack
// through the window host function. The behaviour matches the Rust and
// TypeScript samples (sample-nav, sample-nav-ts): the same texts, the
// same buttons.
//
// The view is written as elements (the JSX runtime's `jsxs`), which is
// what JSX compiles to (the TypeScript sample); the components, the hooks
// and the navigation helpers come from `@pane-app/extension/view`, the
// `popToRoot` host function from `@pane-app/extension/feedback`.
import { popToRoot } from "@pane-app/extension/feedback";
import { jsxs } from "@pane-app/extension/jsx-runtime";
import {
  Button,
  Column,
  Push,
  Row,
  Text,
  createView,
  pop,
  push,
  replace,
  useState,
} from "@pane-app/extension/view";

/** The rows the screen starts at, with what the last popped view answered:
 * each row pushes its detail the Raycast way, `Action.Push`, its `onPop`
 * answering what the detail's pop returned. */
function Rows() {
  const [picked, setPicked] = useState("Nothing picked yet");
  return jsxs(Column, {
    navigationTitle: "Navigation sample",
    gap: "m",
    children: [
      jsxs(Text, { style: "title", level: "primary", children: [picked] }),
      jsxs(Row, {
        gap: "s",
        children: ["One", "Two", "Three"].map((name) =>
          jsxs(Button, {
            onClick: Push(createView(Detail, { name }), (result) =>
              setPicked(`Picked: ${result ?? "nothing"}`),
            ),
            key: name,
            children: [name],
          }),
        ),
      }),
    ],
  });
}

/** The detail of `name`, pushed above the rows. */
function Detail({ name }) {
  return jsxs(Column, {
    navigationTitle: name,
    gap: "m",
    children: [
      jsxs(Text, { style: "title", level: "primary", children: [`Detail: ${name}`] }),
      jsxs(Row, {
        gap: "s",
        children: [
          jsxs(Button, {
            onClick: () => push(createView(Deeper, { name })),
            children: ["Deeper"],
          }),
          jsxs(Button, {
            onClick: () => replace(createView(Deeper, { name })),
            children: ["Swap for deeper"],
          }),
          jsxs(Button, {
            onClick: () => pop(`done:${name}`),
            children: ["Done"],
          }),
        ],
      }),
    ],
  });
}

/** A deeper view, pushed above the detail or replacing it. */
function Deeper({ name }) {
  return jsxs(Column, {
    navigationTitle: "Deeper",
    gap: "m",
    children: [
      jsxs(Text, { style: "title", level: "primary", children: [`Deeper: ${name}`] }),
      jsxs(Row, {
        gap: "s",
        children: [
          jsxs(Button, {
            onClick: () => push(createView(Deeper, { name })),
            children: ["Push another"],
          }),
          jsxs(Button, {
            onClick: () => {
              popToRoot();
            },
            children: ["Pop to root"],
          }),
        ],
      }),
    ],
  });
}

/** The command's one view: the rows, starting with nothing picked. */
export const command = {
  async openView() {
    return createView(Rows);
  },
};
