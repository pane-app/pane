// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's navigation sample in TypeScript: a designed view with a
// navigation stack (#239) — a list-like view whose rows push a detail
// view, which itself pushes deeper; the detail pops with a result the rows
// answer through `onPop`, and the deeper view drops the whole stack
// through the window host function. The behaviour matches the Rust and
// JavaScript samples (sample-nav, sample-nav-js): the same texts, the
// same buttons.
//
// The view is written as JSX: every property of every component is
// type-checked, so a misspelling fails the build (the `jsxImportSource`
// in tsconfig.json makes the transform use the SDK's runtime).
import { popToRoot } from "@pane-app/extension/feedback";
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
import type { Command } from "@pane-app/extension";

/** The detail of a row, by its name. */
interface DetailProps {
  name: string;
}

/** A deeper view, by the name of the row it was reached from. */
interface DeeperProps {
  name: string;
}

/** The rows the screen starts at, with what the last popped view answered:
 * each row pushes its detail the Raycast way, `Action.Push`, its `onPop`
 * answering what the detail's pop returned. */
function Rows() {
  const [picked, setPicked] = useState("Nothing picked yet");
  return (
    <Column navigationTitle="Navigation sample" gap="m">
      <Text style="title" level="primary">
        {picked}
      </Text>
      <Row gap="s">
        {["One", "Two", "Three"].map((name) => (
          <Button
            key={name}
            onClick={Push(
              <Detail name={name} />,
              (result) => setPicked(`Picked: ${result ?? "nothing"}`),
            )}
          >
            {name}
          </Button>
        ))}
      </Row>
    </Column>
  );
}

/** The detail of `name`, pushed above the rows. */
function Detail({ name }: DetailProps) {
  return (
    <Column navigationTitle={name} gap="m">
      <Text style="title" level="primary">
        {`Detail: ${name}`}
      </Text>
      <Row gap="s">
        <Button onClick={() => push(<Deeper name={name} />)}>Deeper</Button>
        <Button onClick={() => replace(<Deeper name={name} />)}>Swap for deeper</Button>
        <Button onClick={() => pop(`done:${name}`)}>Done</Button>
      </Row>
    </Column>
  );
}

/** A deeper view, pushed above the detail or replacing it. */
function Deeper({ name }: DeeperProps) {
  return (
    <Column navigationTitle="Deeper" gap="m">
      <Text style="title" level="primary">
        {`Deeper: ${name}`}
      </Text>
      <Row gap="s">
        <Button onClick={() => push(<Deeper name={name} />)}>Push another</Button>
        <Button onClick={() => popToRoot()}>Pop to root</Button>
      </Row>
    </Column>
  );
}

/** The command's one view: the rows, starting with nothing picked. */
export const command: Command = {
  async openView() {
    return createView(Rows);
  },
};
