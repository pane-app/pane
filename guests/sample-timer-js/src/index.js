// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's timer sample in JavaScript: a clock whose screen changes by
// itself — the view's tree asks Pane to render it again after every
// second (`refresh-after-ms`, #236) — with its caption arriving as pending
// data shown first as a loading state (`usePending`), whose arrival asks
// for a drawing itself (#243): it is shown the moment it lands, with no
// timer to wait for. The behaviour matches the Rust and TypeScript
// samples (sample-timer, sample-timer-ts): the same texts, the same
// timings.
//
// The view is written as elements (the JSX runtime's `jsxs`), which is
// what JSX compiles to (the TypeScript sample); the components and hooks
// come from `@pane-app/extension/view`.
import { jsxs } from "@pane-app/extension/jsx-runtime";
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";
import {
  Column,
  Text,
  createView,
  useInterval,
  usePending,
  useState,
} from "@pane-app/extension/view";

/** What the timer says while its caption is still loading. */
const LOADING = "Loading…";

/** The work that fills the screen when it answers: the timer's caption,
 * held back for a moment — as a service being called would be — so the
 * loading state shows once. */
const loadCaption = () => waitFor(150_000_000).then(() => "A second at a time");

/** The timer the screen shows: a caption loaded before it runs, and the
 * seconds passed since, one per refresh. */
function Timer() {
  const caption = usePending(loadCaption);
  const [elapsed, setElapsed] = useState(0);
  useInterval(1000, () => setElapsed((seconds) => seconds + 1));
  if (caption === undefined) {
    return jsxs(Text, { level: "secondary", children: [LOADING] });
  }
  return jsxs(Column, {
    gap: "m",
    children: [
      jsxs(Text, { level: "secondary", children: [caption] }),
      jsxs(Text, { style: "title", children: [`Elapsed: ${elapsed}s`] }),
    ],
  });
}

/** The command's one view: the timer, loading its caption first. */
export const command = {
  async openView() {
    return createView(Timer);
  },
};
