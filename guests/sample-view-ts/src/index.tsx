// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's designed view sample in TypeScript: a counter whose screen is
// the tree its view's render answers — a column of a text and a row of
// buttons, drawn by Pane, not by the guest (docs/designed-tree.md, ADR
// 0036). The behaviour matches the Rust and JavaScript samples
// (sample-view, sample-view-js): the same text, the same buttons.
//
// The `components` command of the same package answers a gallery of
// every component of the UI component set (#237): the layout primitives,
// the shared components, the tokens and the raw values, with a toggle to
// show the tree changes. Its fields hold state (#238): typing edits at
// once, the view echoes the value back, "Clear" sets it (the
// extension's value wins), and "Reorder" moves the keyed fields around,
// their state with them. And its `loading` command answers a view that
// loads something on open (#243): a loading state drawn at once, and what
// the load answered the moment it arrives — the arrival asks for the
// drawing itself, with no timer to wait for.
//
// The view is written as JSX: every property of every component is
// type-checked, so a misspelling fails the build (the `jsxImportSource`
// in tsconfig.json makes the transform use the SDK's runtime).
import type { Command } from "@pane-app/extension";
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";
import {
  Badge,
  Button,
  Card,
  Checkbox,
  Column,
  Detail,
  Divider,
  EmptyState,
  Grid,
  Icon,
  IconTile,
  Image,
  KeySequence,
  Keycap,
  Link,
  List,
  Loading,
  Markdown,
  MetadataList,
  PasswordInput,
  Progress,
  RichRow,
  Row,
  Scroll,
  Section,
  SectionHeader,
  Segmented,
  Select,
  Slider,
  Spacer,
  Span,
  Stack,
  Tag,
  Text,
  TextArea,
  TextInput,
  Toggle,
  createView,
  selectedKey,
  usePending,
  useState,
} from "@pane-app/extension/view";

/** The counter the screen shows. */
function Counter() {
  const [count, setCount] = useState(0);
  return (
    <Column gap="m">
      <Text style="title" level="primary">
        {`Count: ${count}`}
      </Text>
      <Row gap="s">
        <Button onClick={() => setCount(count + 1)}>Increment</Button>
        <Button onClick={() => setCount(Math.max(0, count - 1))}>Decrement</Button>
        <Button tone="destructive" onClick={() => setCount(0)}>
          Reset
        </Button>
      </Row>
    </Column>
  );
}

/** The gallery of components the `components` command shows. */
function Components() {
  const [on, setOn] = useState(false);
  const [name, setName] = useState("typed");
  const [notes, setNotes] = useState("two lines");
  const [swapped, setSwapped] = useState(false);
  return (
    <Scroll key="gallery" grow={1}>
      <Column gap="l">
        <Text style="heading">The UI component set</Text>
        {/* A stack: a badge over an icon tile, placed. */}
        <Stack place="top-end">
          <IconTile icon={{ builtin: "layers" }} />
          <Badge place="bottom-end" offset={{ x: 4, y: 4 }}>
            4
          </Badge>
        </Stack>
        {/* A text with spans, one a link. */}
        <Text>
          Accept the <Span onClick={() => {}}>terms</Span>
          <Span code> before continuing.</Span>
        </Text>
        <Row name="Marks" gap="s">
          <Icon icon="star" size="l" />
          {/* A raw blue, corrected for contrast. */}
          <Icon icon={{ builtin: "bell", tint: "#88ccff" }} />
          <Keycap>ctrl</Keycap>
          <KeySequence keys={["ctrl", "shift", "p"]} />
          <Tag color="blue">beta</Tag>
          <Badge>3</Badge>
        </Row>
        <Card gap="s">
          <RichRow
            subtitle="A tree Pane renders"
            icon={{ builtin: "layers" }}
            accessories={[{ text: "new", tag: true }]}
            onClick={() => {}}
          >
            Pane
          </RichRow>
        </Card>
        {/* Controls, one of them live. */}
        <Column gap="s">
          <SectionHeader note="Every one focusable">Controls</SectionHeader>
          <Toggle on={on} label="Dark mode" onChange={() => setOn(!on)} />
          <Checkbox on={on} label="Remember" onChange={() => setOn(!on)} />
          <Segmented
            options={[
              { value: "daily", label: "Daily" },
              { value: "weekly", label: "Weekly" },
            ]}
            value="daily"
            label="Digest"
            onChange={() => {}}
          />
          <Select
            options={[
              { value: "daily", label: "Daily" },
              { value: "weekly", label: "Weekly" },
            ]}
            value="daily"
            label="Pick"
            onChange={() => {}}
          />
          <Slider value={0.4} label="Volume" onChange={() => {}} />
          <Progress value={0.7} label="Installed" />
          <Loading label="Checking" />
        </Column>
        {/* The fields, live and keyed (#238): the name field hears its
            value as the user types (the view echoing it back, which never
            fights the typing), the notes field on its commits, "Clear"
            sets both (the extension's value replacing the text), and
            "Reorder" swaps the two fields, whose keys keep their state. */}
        <Column gap="s">
          <SectionHeader note="Live, keyed">Fields</SectionHeader>
          {/* "Reorder" swaps the fields' places, not their state: the keys
              keep each field's text, caret and focus. */}
          {swapped ? (
            <>
              <TextArea key="notes" value={notes} label="Notes" onChange={setNotes} />
              <TextInput
                key="name"
                value={name}
                placeholder="Type here"
                label="Name"
                onInput={setName}
              />
            </>
          ) : (
            <>
              <TextInput
                key="name"
                value={name}
                placeholder="Type here"
                label="Name"
                onInput={setName}
              />
              <TextArea key="notes" value={notes} label="Notes" onChange={setNotes} />
            </>
          )}
          <Row gap="s">
            <Button
              onClick={() => {
                setName("");
                setNotes("");
              }}
            >
              Clear
            </Button>
            <Button onClick={() => setSwapped(!swapped)}>Reorder</Button>
          </Row>
          <Text>{`Echo: ${name}`}</Text>
          <PasswordInput label="Secret" />
        </Column>
        {/* Markdown. */}
        <Markdown>
          {"# Markdown\n\nSome *prose*, `code` and [a link](https://pane.dev).\n\n- [x] drawn\n- [ ] still to do\n"}
        </Markdown>
        <MetadataList
          items={[
            { label: "Author", value: "Vu", onClick: () => {} },
            { label: "Tags", tags: ["one", "two"] },
            { separator: true },
            { label: "Kind", value: "sample" },
          ]}
        />
        <EmptyState title="Nothing here" description="The gallery is over" icon={{ builtin: "search-minus" }}>
          <Button onClick={() => {}}>Start over</Button>
        </EmptyState>
        {/* An image, with a placeholder while it stands in. */}
        <Image image={{ builtin: "image" }} size="xl" fit="cover">
          <Text level="tertiary">Loading…</Text>
        </Image>
        {/* Raw values: a surface with a variant, a tone, a corrected
            colour and an exact one. */}
        <Row gap="s">
          <Text level="secondary" background="danger" radius="m" hover={{ background: "accent" }}>
            Surface
          </Text>
          <Link onClick={() => {}}>A link</Link>
        </Row>
        <Row gap="s">
          <Text color="#88ccff">
            Corrected
          </Text>
          <Text color={{ raw: "#ff6363" }}>Exact</Text>
        </Row>
        <Divider />
        <Spacer />
      </Column>
    </Scroll>
  );
}

/** What the loading sample loads: held back for a moment, as work from a
 * service would be, so the loading state shows once. */
const load = (): Promise<string> =>
  waitFor(300_000_000).then(() => "Pane drew this the moment it arrived");

/** The loading sample: a loading state drawn at once, and what the load
 * answered the moment it arrives. */
function LoadingView() {
  const what = usePending(load);
  if (what === undefined) {
    return <Text level="secondary">Loading…</Text>;
  }
  return (
    <Column gap="m">
      <Text style="title">Loaded</Text>
      <Text level="secondary">{what}</Text>
    </Column>
  );
}

/** The List sample (#240): sections, keywords, accessories, a
 * host-filtered search field, the empty view, the detail pane (built for
 * the selected item, as the render context names it), the search-bar
 * dropdown, the loading bar and pagination. The first item's press
 * toggles the loading bar, so its 300 ms threshold can be seen. */
function NotesView() {
  const [listLoading, setListLoading] = useState(true);
  const [pages, setPages] = useState(1);
  const [pinned, setPinned] = useState(false);
  const selected = selectedKey();
  const note = (key: string, title: string, subtitle: string, keyword: string, tag: string, color: string) => (
    <List.Item
      key={key}
      title={title}
      subtitle={subtitle}
      icon={{ builtin: "document" }}
      keywords={[keyword]}
      accessories={[{ tag, color }, { text: "1" }]}
      onClick={() => setListLoading(!listLoading)}
      actions={[{ title: "Copy", onClick: () => {} }]}
      detail={
        selected === key ? (
          <Detail>
            <Markdown>{`# ${title}\n\n*Selected:* ${key} \u2014 the pane's content is built for it alone.\n\n- [x] The render context names it\n- [ ] No other item's detail is built\n\n| Field | Value |\n| --- | --- |\n| key | ${key} |\n| title | ${title} |\n\n![Pane](data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 12'%3E%3Crect width='24' height='12' rx='3' fill='%233d5afe'/%3E%3C/svg%3E =48x24 "Pane's mark")\n\nSee [the tree](https://example.com/tree).`}</Markdown>
          </Detail>
        ) : undefined
      }
    >
      {title}
    </List.Item>
  );
  const sections = [
    <List.Section key="notes" title="Notes" subtitle="Three notes">
      {note("first", "First note", "The first of the notes", "opening", "new", "green")}
      {note("second", "Second note", "The second of the notes", "middle", "kept", "blue")}
      {note("third", "Third note", "The third of the notes", "closing", "done", "red")}
    </List.Section>,
  ];
  if (pinned) {
    sections.push(
      <List.Section key="pinned" title="Pinned">
        {note("pinned", "Pinned note", "The one that is pinned", "kept", "pinned", "yellow")}
      </List.Section>,
    );
  } else {
    sections.push(
      <List.Section key="more" title="More" subtitle="A page at a time">
        {Array.from({ length: pages * 6 }, (_, at) => (
          <List.Item
            key={`more-${at}`}
            title={`More ${at}`}
            subtitle="One page of a longer list"
            keywords={["page"]}
            onClick={() => {}}
          >
            {`More ${at}`}
          </List.Item>
        ))}
      </List.Section>,
    );
  }
  return (
    <List
      navigationTitle="Notes"
      searchPlaceholder="Search notes…"
      isLoading={listLoading}
      isShowingDetail={true}
      hasMore={pages < 3}
      pageSize={6}
      onLoadMore={() => setPages(pages + 1)}
    >
      <List.Dropdown value={pinned ? "pinned" : "all"} onChange={() => setPinned(!pinned)}>
        {[{ value: "all", title: "All notes" }, { value: "pinned", title: "Pinned" }]}
      </List.Dropdown>
      {sections}
      <EmptyState title="No notes" description="Nothing matches the search.">
        <Button onClick={() => {}}>Clear the search</Button>
      </EmptyState>
    </List>
  );
}

/** The Grid sample (#240): cells of images and colours in two sections,
 * each with its own columns, aspect ratio, fit and inset. */
function CellsView() {
  return (
    <Grid navigationTitle="Cells" searchPlaceholder="Search cells…">
      <Grid.Section title="Warm" columns={3} aspectRatio={2} fit="cover">
        <Grid.Item key="warm-0" title="Amber" subtitle="A colour cell" color="#ffb300" onClick={() => {}} />
        <Grid.Item key="warm-1" title="Coral" subtitle="A colour cell" color="#ff7043" onClick={() => {}} />
        <Grid.Item key="warm-2" title="Document" subtitle="An image cell" image={{ builtin: "document" }} onClick={() => {}} />
      </Grid.Section>
      <Grid.Section title="Cool" columns={4} aspectRatio={1} inset={true}>
        <Grid.Item key="cool-0" title="Indigo" subtitle="A colour cell" color="#3d5afe" onClick={() => {}} />
        <Grid.Item key="cool-1" title="Teal" subtitle="A colour cell" color="#00897b" onClick={() => {}} />
        <Grid.Item key="cool-2" title="Star" subtitle="An image cell" image={{ builtin: "star" }} onClick={() => {}} />
        <Grid.Item key="cool-3" title="Typed" subtitle="A subtree cell" onClick={() => {}}>
          <Text level="secondary">A cell of the author's own</Text>
        </Grid.Item>
      </Grid.Section>
    </Grid>
  );
}

/** The Detail sample (#240): Markdown with an image, a task list and a
 * table, a metadata panel and actions. */
function AboutView() {
  return (
    <Detail navigationTitle="About Pane">
      <Markdown>{`# Pane\n\nA screen an extension designs as a **tree** Pane renders.\n\n![Pane](data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 12'%3E%3Crect width='24' height='12' rx='3' fill='%233d5afe'/%3E%3C/svg%3E =48x24 "Pane's mark")\n\n- [x] Drawn by Pane\n- [ ] Drawn by the extension\n\n| Field | Value |\n| --- | --- |\n| version | 0.1 |\n| renderer | GPUI |\n\nSee [the tree](https://example.com/tree).`}</Markdown>
      <MetadataList
        items={[
          { label: "Version", value: "0.1.0" },
          { label: "Tags", tags: ["designed", "tree"] },
          { separator: true },
          { label: "Repository", value: "pane", onClick: () => {} },
        ]}
      />
      <Row gap="s">
        <Button onClick={() => {}}>Done</Button>
      </Row>
    </Detail>
  );
}

export const command: Command = {
  async openView(commandId: string) {
    if (commandId === "sample") {
      return createView(Counter);
    }
    if (commandId === "components") {
      return createView(Components);
    }
    if (commandId === "loading") {
      return createView(LoadingView);
    }
    if (commandId === "list") {
      return createView(NotesView);
    }
    if (commandId === "grid") {
      return createView(CellsView);
    }
    if (commandId === "detail") {
      return createView(AboutView);
    }
    throw new Error("this command opens no designed view");
  },
};
