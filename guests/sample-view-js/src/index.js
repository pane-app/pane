// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's designed view sample in JavaScript: a counter whose screen is
// the tree its view's render answers — a column of a text and a row of
// buttons, drawn by Pane, not by the guest (docs/designed-tree.md, ADR
// 0036). The behaviour matches the Rust and TypeScript samples
// (sample-view, sample-view-ts): the same text, the same buttons.
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
// The view is written as elements (the JSX runtime's `jsxs`), which is
// what JSX compiles to (the TypeScript sample); the components and hooks
// come from `@pane-app/extension/view`.
import { jsxs } from "@pane-app/extension/jsx-runtime";
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
  return jsxs(Column, {
    gap: "m",
    children: [
      jsxs(Text, { style: "title", level: "primary", children: [`Count: ${count}`] }),
      jsxs(Row, {
        gap: "s",
        children: [
          jsxs(Button, {
            onClick: () => setCount(count + 1),
            children: ["Increment"],
          }),
          jsxs(Button, {
            onClick: () => setCount(Math.max(0, count - 1)),
            children: ["Decrement"],
          }),
          jsxs(Button, {
            tone: "destructive",
            onClick: () => setCount(0),
            children: ["Reset"],
          }),
        ],
      }),
    ],
  });
}

/** The gallery of components the `components` command shows. */
function Components() {
  const [on, setOn] = useState(false);
  const [name, setName] = useState("typed");
  const [notes, setNotes] = useState("two lines");
  const [swapped, setSwapped] = useState(false);
  const nameField = jsxs(TextInput, {
    key: "name",
    value: name,
    placeholder: "Type here",
    label: "Name",
    onInput: (value) => setName(value),
  });
  const notesField = jsxs(TextArea, {
    key: "notes",
    value: notes,
    label: "Notes",
    onChange: (value) => setNotes(value),
  });
  return jsxs(Scroll, { key: "gallery", grow: 1,
    children: [
      jsxs(Column, {
        gap: "l",
        children: [
          jsxs(Text, { style: "heading", children: ["The UI component set"] }),
          // A stack: a badge over an icon tile, placed.
          jsxs(Stack, {
            place: "top-end",
            children: [
              jsxs(IconTile, { icon: { builtin: "layers" } }),
              jsxs(Badge, {
                place: "bottom-end",
                offset: { x: 4, y: 4 },
                children: ["4"],
              }),
            ],
          }),
          // A text with spans, one a link.
          jsxs(Text, {
            children: [
              "Accept the ",
              jsxs(Span, { onClick: () => {}, children: ["terms"] }),
              jsxs(Span, { code: true, children: [" before continuing."] }),
            ],
          }),
          jsxs(Row, {
            name: "Marks",
            gap: "s",
            children: [
              jsxs(Icon, { icon: "star", size: "l" }),
              // A raw blue, corrected for contrast.
              jsxs(Icon, { icon: { builtin: "bell", tint: "#88ccff" } }),
              jsxs(Keycap, { children: ["ctrl"] }),
              jsxs(KeySequence, { keys: ["ctrl", "shift", "p"] }),
              jsxs(Tag, { color: "blue", children: ["beta"] }),
              jsxs(Badge, { children: ["3"] }),
            ],
          }),
          jsxs(Card, {
            gap: "s",
            children: [
              jsxs(RichRow, {
                subtitle: "A tree Pane renders",
                icon: { builtin: "layers" },
                accessories: [{ text: "new", tag: true }],
                onClick: () => {},
                children: ["Pane"],
              }),
            ],
          }),
          // Controls, one of them live.
          jsxs(Column, {
            gap: "s",
            children: [
              jsxs(SectionHeader, {
                note: "Every one focusable",
                children: ["Controls"],
              }),
              jsxs(Toggle, {
                on,
                label: "Dark mode",
                onChange: () => setOn(!on),
              }),
              jsxs(Checkbox, {
                on,
                label: "Remember",
                onChange: () => setOn(!on),
              }),
              jsxs(Segmented, {
                options: [
                  { value: "daily", label: "Daily" },
                  { value: "weekly", label: "Weekly" },
                ],
                value: "daily",
                label: "Digest",
                onChange: () => {},
              }),
              jsxs(Select, {
                options: [
                  { value: "daily", label: "Daily" },
                  { value: "weekly", label: "Weekly" },
                ],
                value: "daily",
                label: "Pick",
                onChange: () => {},
              }),
              jsxs(Slider, { value: 0.4, label: "Volume", onChange: () => {} }),
              jsxs(Progress, { value: 0.7, label: "Installed" }),
              jsxs(Loading, { label: "Checking" }),
            ],
          }),
          // The fields, live and keyed (#238): the name field hears its
          // value as the user types (the view echoing it back, which
          // never fights the typing), the notes field on its commits,
          // "Clear" sets both (the extension's value replacing the text),
          // and "Reorder" swaps the two fields, whose keys keep their
          // state.
          jsxs(Column, {
            gap: "s",
            children: [
              jsxs(SectionHeader, { note: "Live, keyed", children: ["Fields"] }),
              // "Reorder" swaps the fields' places, not their state: the
              // keys keep each field's text, caret and focus.
              ...(swapped ? [notesField, nameField] : [nameField, notesField]),
              jsxs(Row, {
                gap: "s",
                children: [
                  jsxs(Button, {
                    onClick: () => {
                      setName("");
                      setNotes("");
                    },
                    children: ["Clear"],
                  }),
                  jsxs(Button, {
                    onClick: () => setSwapped(!swapped),
                    children: ["Reorder"],
                  }),
                ],
              }),
              jsxs(Text, { children: [`Echo: ${name}`] }),
              jsxs(PasswordInput, { label: "Secret" }),
            ],
          }),
          // Markdown.
          jsxs(Markdown, {
            children: [
              "# Markdown\n\nSome *prose*, `code` and [a link](https://pane.dev).\n\n- [x] drawn\n- [ ] still to do\n",
            ],
          }),
          jsxs(MetadataList, {
            items: [
              { label: "Author", value: "Vu", onClick: () => {} },
              { label: "Tags", tags: ["one", "two"] },
              { separator: true },
              { label: "Kind", value: "sample" },
            ],
          }),
          jsxs(EmptyState, {
            title: "Nothing here",
            description: "The gallery is over",
            icon: { builtin: "search-minus" },
            children: [jsxs(Button, { onClick: () => {}, children: ["Start over"] })],
          }),
          // An image, with a placeholder while it stands in.
          jsxs(Image, {
            image: { builtin: "image" },
            size: "xl",
            fit: "cover",
            children: [jsxs(Text, { level: "tertiary", children: ["Loading…"] })],
          }),
          // Raw values: a surface with a variant, a tone, a corrected
          // colour and an exact one.
          jsxs(Row, {
            gap: "s",
            children: [
              jsxs(Text, {
                level: "secondary",
                background: "danger",
                radius: "m",
                hover: { background: "accent" },
                children: ["Surface"],
              }),
              jsxs(Link, { onClick: () => {}, children: ["A link"] }),
            ],
          }),
          jsxs(Row, {
            gap: "s",
            children: [
              jsxs(Text, { color: "#88ccff", children: ["Corrected"] }),
              jsxs(Text, { color: { raw: "#ff6363" }, children: ["Exact"] }),
            ],
          }),
          jsxs(Divider, {}),
          jsxs(Spacer, {}),
        ],
      }),
    ],
  });
}

/** What the loading sample loads: held back for a moment, as work from a
 * service would be, so the loading state shows once. */
const load = () => waitFor(300_000_000).then(() => "Pane drew this the moment it arrived");

/** The loading sample: a loading state drawn at once, and what the load
 * answered the moment it arrives. */
function LoadingView() {
  const what = usePending(load);
  if (what === undefined) {
    return jsxs(Text, { level: "secondary", children: ["Loading…"] });
  }
  return jsxs(Column, {
    gap: "m",
    children: [
      jsxs(Text, { style: "title", children: ["Loaded"] }),
      jsxs(Text, { level: "secondary", children: [what] }),
    ],
  });
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
  const note = (key, title, subtitle, keyword, tag, color) =>
    jsxs(List.Item, {
      key,
      title,
      subtitle,
      icon: { builtin: "document" },
      keywords: [keyword],
      accessories: [{ tag, color }, { text: "1" }],
      onClick: () => setListLoading(!listLoading),
      actions: [{ title: "Copy", onClick: () => {} }],
      detail:
        selected === key
          ? jsxs(Detail, {
              children: [
                jsxs(Markdown, {
                  children: [
                    `# ${title}\n\n*Selected:* ${key} \u2014 the pane's content is built for it alone.\n\n- [x] The render context names it\n- [ ] No other item's detail is built\n\n| Field | Value |\n| --- | --- |\n| key | ${key} |\n| title | ${title} |\n\n![Pane](data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 12'%3E%3Crect width='24' height='12' rx='3' fill='%233d5afe'/%3E%3C/svg%3E =48x24 "Pane's mark")\n\nSee [the tree](https://example.com/tree).`,
                  ],
                }),
              ],
            })
          : undefined,
    });
  const sections = [
    jsxs(List.Section, {
      title: "Notes",
      subtitle: "Three notes",
      children: [
        note("first", "First note", "The first of the notes", "opening", "new", "green"),
        note("second", "Second note", "The second of the notes", "middle", "kept", "blue"),
        note("third", "Third note", "The third of the notes", "closing", "done", "red"),
      ],
    }),
  ];
  if (pinned) {
    sections.push(
      jsxs(List.Section, {
        title: "Pinned",
        children: [note("pinned", "Pinned note", "The one that is pinned", "kept", "pinned", "yellow")],
      }),
    );
  } else {
    sections.push(
      jsxs(List.Section, {
        title: "More",
        subtitle: "A page at a time",
        children: Array.from({ length: pages * 6 }, (_, at) =>
          jsxs(List.Item, {
            key: `more-${at}`,
            title: `More ${at}`,
            subtitle: "One page of a longer list",
            keywords: ["page"],
            onClick: () => {},
          }),
        ),
      }),
    );
  }
  return jsxs(List, {
    navigationTitle: "Notes",
    searchPlaceholder: "Search notes…",
    isLoading: listLoading,
    isShowingDetail: true,
    hasMore: pages < 3,
    pageSize: 6,
    onLoadMore: () => setPages(pages + 1),
    children: [
      jsxs(List.Dropdown, {
        value: pinned ? "pinned" : "all",
        onChange: () => setPinned(!pinned),
        children: [
          { value: "all", title: "All notes" },
          { value: "pinned", title: "Pinned" },
        ],
      }),
      ...sections,
      jsxs(EmptyState, {
        title: "No notes",
        description: "Nothing matches the search.",
        children: [jsxs(Button, { onClick: () => {}, children: ["Clear the search"] })],
      }),
    ],
  });
}

/** The Grid sample (#240): cells of images and colours in two sections,
 * each with its own columns, aspect ratio, fit and inset. */
function CellsView() {
  return jsxs(Grid, {
    navigationTitle: "Cells",
    searchPlaceholder: "Search cells…",
    children: [
      jsxs(Grid.Section, {
        title: "Warm",
        columns: 3,
        aspectRatio: 2,
        fit: "cover",
        children: [
          jsxs(Grid.Item, {
            key: "warm-0",
            title: "Amber",
            subtitle: "A colour cell",
            color: "#ffb300",
            onClick: () => {},
          }),
          jsxs(Grid.Item, {
            key: "warm-1",
            title: "Coral",
            subtitle: "A colour cell",
            color: "#ff7043",
            onClick: () => {},
          }),
          jsxs(Grid.Item, {
            key: "warm-2",
            title: "Document",
            subtitle: "An image cell",
            image: { builtin: "document" },
            onClick: () => {},
          }),
        ],
      }),
      jsxs(Grid.Section, {
        title: "Cool",
        columns: 4,
        aspectRatio: 1,
        inset: true,
        children: [
          jsxs(Grid.Item, {
            key: "cool-0",
            title: "Indigo",
            subtitle: "A colour cell",
            color: "#3d5afe",
            onClick: () => {},
          }),
          jsxs(Grid.Item, {
            key: "cool-1",
            title: "Teal",
            subtitle: "A colour cell",
            color: "#00897b",
            onClick: () => {},
          }),
          jsxs(Grid.Item, {
            key: "cool-2",
            title: "Star",
            subtitle: "An image cell",
            image: { builtin: "star" },
            onClick: () => {},
          }),
          jsxs(Grid.Item, {
            key: "cool-3",
            title: "Typed",
            subtitle: "A subtree cell",
            children: [jsxs(Text, { level: "secondary", children: ["A cell of the author's own"] })],
            onClick: () => {},
          }),
        ],
      }),
    ],
  });
}

/** The Detail sample (#240): Markdown with an image, a task list and a
 * table, a metadata panel and actions. */
function AboutView() {
  return jsxs(Detail, {
    navigationTitle: "About Pane",
    children: [
      jsxs(Markdown, {
        children: [
          "# Pane\n\nA screen an extension designs as a **tree** Pane renders.\n\n![Pane](data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 12'%3E%3Crect width='24' height='12' rx='3' fill='%233d5afe'/%3E%3C/svg%3E =48x24 \"Pane's mark\")\n\n- [x] Drawn by Pane\n- [ ] Drawn by the extension\n\n| Field | Value |\n| --- | --- |\n| version | 0.1 |\n| renderer | GPUI |\n\nSee [the tree](https://example.com/tree).",
        ],
      }),
      jsxs(MetadataList, {
        items: [
          { label: "Version", value: "0.1.0" },
          { label: "Tags", tags: ["designed", "tree"] },
          { separator: true },
          { label: "Repository", value: "pane", onClick: () => {} },
        ],
      }),
      jsxs(Row, { gap: "s", children: [jsxs(Button, { onClick: () => {}, children: ["Done"] })] }),
    ],
  });
}

/** The command's views: the counter, the gallery, the loading sample, or
 * one of the standard views (#240). */
export const command = {
  async openView(commandId) {
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
