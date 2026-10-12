// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Package search, the online search sample, in JavaScript: a command that
// searches a web service as the user types into its own search field. Pane
// asks it only once the user has opened it, never while they type in root
// search. The service is the fixture service, a made-up package registry on
// this computer (`cargo run -p pane-core --example fixture_service`, port
// 8740 by default); the command reaches it with `get` from
// `@pane-app/extension/http` (`wasi:http` underneath) and its address is a
// setting the command's form changes. A search Pane no longer needs is
// stopped where it waits; an unreachable or failing service is an error
// shown in place of results, not a crash. Items, toasts and errors match
// the Rust sample (guests/sample-search) and the TypeScript one.

import { showToast } from "@pane-app/extension/feedback";
import { get as fetchUrl } from "@pane-app/extension/http";
import {
  Button,
  Column,
  EmptyState,
  List,
  TextInput,
  Push,
  createView,
  pop,
  useRef,
  useState,
} from "@pane-app/extension/view";
import { get, set } from "pane:extension/settings@0.1.0";
import { jsxs } from "@pane-app/extension/jsx-runtime";

/** The address used until the user sets another. */
const DEFAULT_SERVICE = "http://127.0.0.1:8740";
/** The settings key holding the service address. */
const SERVICE = "service";
/** The command's id in pane.json. */
const COMMAND = "packages";

/**
 * Calls a host function, turning its error into a plain message.
 * @template T
 * @param {() => T} call
 * @returns {T}
 */
function host(call) {
  try {
    return call();
  } catch (error) {
    const payload = /** @type {{ payload?: unknown }} */ (error).payload;
    throw typeof payload === "string" ? new Error(payload) : error;
  }
}

/** The service address: the saved one, or the default. */
function service() {
  return host(() => get(SERVICE)) ?? DEFAULT_SERVICE;
}

/**
 * Fetches `path` from the service and reads its JSON answer.
 * @param {string} path
 * @returns {Promise<any>}
 */
async function fetchJson(path) {
  const address = service();
  let response;
  try {
    response = await fetchUrl(`${address}${path}`, { accept: "application/json" });
  } catch (error) {
    throw new Error(`Could not reach the service at ${address}: ${/** @type {Error} */ (error).message}`);
  }
  if (response.status !== 200) {
    let why = response.text();
    try {
      why = /** @type {{ error: string }} */ (response.json()).error ?? why;
    } catch {
      // Not JSON: the body as it is.
    }
    throw new Error(`The service answered ${response.status}: ${why}`);
  }
  try {
    return response.json();
  } catch (error) {
    throw new Error(`The service's answer could not be read: ${/** @type {Error} */ (error).message}`);
  }
}

/**
 * Runs the action `itemId`: the "about" item's, or a search result's
 * ("package:<name>"), which fetches that package's details; it shows a
 * toast with what it found.
 * @param {string} itemId
 * @returns {Promise<void>}
 */
/**
 * The text the action `itemId`'s toast shows.
 * @param {string} itemId
 * @returns {Promise<string>}
 */
/**
 * Searches `query` with the service, as the view's List asks for it.
 * @param {string} query
 * @returns {Promise<{ results: { name: string, summary: string }[] }>}
 */
async function search(query) {
  return fetchJson(`/search?q=${encodeURIComponent(query)}`);
}

/**
 * The text the action `itemId`'s toast shows.
 * @param {string} itemId
 * @returns {Promise<string>}
 */
async function outcome(itemId) {
  if (!itemId.startsWith("package:")) {
    throw new Error(`unknown item: ${itemId}`);
  }
  const name = itemId.slice("package:".length);
  const details = await fetchJson(`/packages/${encodeURIComponent(name)}`);
  return `${details.name} ${details.version} (${details.license}): ${details.summary}`;
}

/** The search view: a List that handles its search itself (#240), the
 * service's answers as its items. The search the text starts runs in the
 * event that hears it, and the results are drawn the moment they land;
 * the loading state shows while it runs. The "Service address" item
 * pushes a view of a text field and a Save button, the modern replacement
 * for the typed form the List document carried. */
function Packages() {
  const [results, setResults] = useState(/** @type {{ name: string, summary: string }[]} */ ([]));
  const [failed, setFailed] = useState(/** @type {string | null} */ (null));
  const [loading, setLoading] = useState(false);
  const searching = useRef(/** @type {{ run: Promise<{ results: { name: string, summary: string }[] }> } | null} */ (null));
  /** @param {string} text */
  const onSearchText = async (text) => {
    if (!text.trim()) {
      setResults([]);
      setFailed(null);
      setLoading(false);
      return;
    }
    setLoading(true);
    const held = { run: search(text) };
    searching.current = held;
    try {
      const found = await held.run;
      if (searching.current !== held) return;
      setResults(found.results);
      setFailed(null);
    } catch (error) {
      if (searching.current !== held) return;
      setResults([]);
      setFailed(String(/** @type {Error} */ (error).message ?? error));
    } finally {
      if (searching.current === held) setLoading(false);
    }
  };
  const items = results.map((/** @type {{ name: string, summary: string }} */ pkg) =>
    jsxs(List.Item, {
      key: `package:${pkg.name}`,
      title: pkg.name,
      subtitle: pkg.summary,
      onClick: async () => {
        showToast({ title: await outcome(`package:${pkg.name}`) });
      },
      children: [pkg.name],
    }),
  );
  return jsxs(List, {
    navigationTitle: "Package search",
    searchPlaceholder: "Search the registry…",
    isLoading: loading,
    onSearchText,
    children: [
      ...items,
      jsxs(EmptyState, {
        title: /** @type {string} */ (failed ?? "Type to search the package registry"),
        description: "Results come from the service as you type; Enter shows a package's details",
        children: [
          jsxs(Button, {
            onClick: Push(jsxs(AddressView, { children: [] })),
            children: ["Service address"],
          }),
        ],
      }),
    ],
  });
}

/** The service address view, pushed above the list: a text field and a
 * Save button. */
function AddressView() {
  const [address, setAddress] = useState(/** @type {string} */ (service()));
  return jsxs(Column, {
    navigationTitle: "Service address",
    gap: "m",
    children: [
      jsxs(TextInput, {
        key: "address",
        label: "Address",
        placeholder: DEFAULT_SERVICE,
        value: address,
        onInput: (/** @type {string} */ value) => setAddress(value),
        children: [address],
      }),
      jsxs(Button, {
        onClick: () =>
          pop(`Searching ${address.trim().replace(/\/+$/, "")} from now on`),
        children: ["Save"],
      }),
    ],
  });
}

/** @type {import("@pane-app/extension").Command} */
export const command = {
  async openView(commandId) {
    if (commandId !== COMMAND) {
      throw new Error(`unknown command: ${commandId}`);
    }
    return createView(Packages);
  },
};
