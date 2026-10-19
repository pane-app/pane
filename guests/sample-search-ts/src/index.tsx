// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Package search, the online search sample, in TypeScript: a command that
// searches a web service as the user types into its own search field. Pane
// asks it only once the user has opened it, never while they type in root
// search. The service is the fixture service, a made-up package registry on
// this computer (`cargo run -p pane-core --example fixture_service`, port
// 8740 by default); the command reaches it with `get` from
// `@pane-app/extension/http` (`wasi:http` underneath) and its address is a
// setting the command's form changes. A search Pane no longer needs is
// stopped where it waits; an unreachable or failing service is an error
// shown in place of results, not a crash. Items, toasts and errors match
// the Rust sample (guests/sample-search) and the JavaScript one.
import type { Command } from "@pane-app/extension";
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
import { getPreferenceValues } from "@pane-app/extension/preferences";
import { get, set } from "pane:extension/settings@0.2.0";

/** The address used until the user sets another. */
const DEFAULT_SERVICE = "http://127.0.0.1:8740";
/** The settings key holding the service address. */
const SERVICE = "service";
/** The command's id in pane.json. */
const COMMAND = "packages";

interface Found {
  results: { name: string; summary: string }[];
}

interface Details {
  name: string;
  summary: string;
  version: string;
  license: string;
}

/** Calls a host function, turning its error into a plain message. */
function host<T>(call: () => T): T {
  try {
    return call();
  } catch (error) {
    const payload = (error as { payload?: unknown }).payload;
    throw typeof payload === "string" ? new Error(payload) : error;
  }
}

/** The service address: the one the user set as the command's `service`
 * preference (Setup, or the extension's card in Settings), the one this
 * command saved on its Service address view, or the default. */
function service(): string {
  let user: string | undefined;
  try {
    user = getPreferenceValues().service as string | undefined;
  } catch {
    // Pane refused (the code is stopped): the saved address stands.
  }
  return user ?? host(() => get(SERVICE)) ?? DEFAULT_SERVICE;
}

/** Fetches `path` from the service and reads its JSON answer. */
async function fetchJson<T>(path: string): Promise<T> {
  const address = service();
  let response;
  try {
    response = await fetchUrl(`${address}${path}`, { accept: "application/json" });
  } catch (error) {
    throw new Error(`Could not reach the service at ${address}: ${(error as Error).message}`);
  }
  if (response.status !== 200) {
    let why = response.text();
    try {
      why = (response.json() as { error?: string }).error ?? why;
    } catch {
      // Not JSON: the body as it is.
    }
    throw new Error(`The service answered ${response.status}: ${why}`);
  }
  try {
    return response.json() as T;
  } catch (error) {
    throw new Error(`The service's answer could not be read: ${(error as Error).message}`);
  }
}

/**
 * Runs the action `itemId`: the "about" item's, or a search result's
 * ("package:<name>"), which fetches that package's details; it shows a
 * toast with what it found.
 */
/** The text the action `itemId`'s toast shows. */
async function outcome(itemId: string): Promise<string> {
  if (itemId === "about") {
    return "Type in the search field to search the package registry";
  }
  if (!itemId.startsWith("package:")) {
    throw new Error(`unknown item: ${itemId}`);
  }
  const name = itemId.slice("package:".length);
  const details = await fetchJson<Details>(`/packages/${encodeURIComponent(name)}`);
  return `${details.name} ${details.version} (${details.license}): ${details.summary}`;
}

/** Searches `query` with the service, as the view's List asks for it. */
async function search(query: string): Promise<Found> {
  return fetchJson<Found>(`/search?q=${encodeURIComponent(query)}`);
}

/** The search view: a List that handles its search itself (#240), the
 * service's answers as its items. The search the text starts runs past
 * the event that hears it — the event answers once its own synchronous
 * part has started the search, so a newer one's answer is never made to
 * wait behind it — and the results are drawn the moment they land, the
 * arrival asking for its own drawing; the loading state shows while it
 * runs. The "Service address" item pushes a view of a text field and a
 * Save button, the modern replacement for the typed form the List
 * document carried. */
function Packages() {
  const [results, setResults] = useState<{ name: string; summary: string }[]>([]);
  const [failed, setFailed] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const searching = useRef<{ run: Promise<Found> } | null>(null);
  const onSearchText = async (text: string) => {
    if (!text.trim()) {
      setResults([]);
      setFailed(null);
      setLoading(false);
      return;
    }
    // A newer search starts: the rows the old one drew are cleared here,
    // in the listener's synchronous part, as the Rust sample's render
    // clears them when its query changes — the loading state the event's
    // answer draws lists nothing stale, and the answer's own drawing
    // fills the rows in when it lands.
    setResults([]);
    setFailed(null);
    setLoading(true);
    const run = search(text);
    const held = { run };
    searching.current = held;
    try {
      const found = await run;
      if (searching.current !== held) return;
      setResults(found.results);
      setFailed(null);
    } catch (error) {
      if (searching.current !== held) return;
      setResults([]);
      setFailed(String((error as Error).message ?? error));
    } finally {
      if (searching.current === held) setLoading(false);
    }
  };
  return (
    <List
      navigationTitle="Package search"
      searchPlaceholder="Search the registry…"
      isLoading={loading}
      onSearchText={onSearchText}
    >
      {results.map((pkg) => (
        <List.Item
          key={`package:${pkg.name}`}
          id={`package:${pkg.name}`}
          title={pkg.name}
          subtitle={pkg.summary}
          onClick={async () => {
            showToast({ title: await outcome(`package:${pkg.name}`) });
          }}
        >
          {pkg.name}
        </List.Item>
      ))}
      <EmptyState
        title={failed ?? "Type to search the package registry"}
        description="Results come from the service as you type; Enter shows a package's details"
        // A failed search draws as one: the warning sign, in the theme's
        // danger colour, beside the service's answer.
        icon={failed ? { builtin: "warning", filled: true, tint: "danger" } : undefined}
      >
        <Button onClick={Push(<AddressView />)}>Service address</Button>
      </EmptyState>
    </List>
  );
}

/** The service address view, pushed above the list: a text field and a
 * Save button. */
function AddressView() {
  const [address, setAddress] = useState(service());
  return (
    <Column navigationTitle="Service address" gap="m">
      <TextInput
        key="address"
        label="Address"
        placeholder={DEFAULT_SERVICE}
        value={address}
        onInput={(value: string) => setAddress(value)}
      >
        {address}
      </TextInput>
      <Button onClick={() => pop(`Searching ${address.trim().replace(/\/+$/, "")} from now on`)}>
        Save
      </Button>
    </Column>
  );
}

export const command: Command = {
  async openView(commandId: string) {
    if (commandId !== COMMAND) {
      throw new Error(`unknown command: ${commandId}`);
    }
    return createView(Packages);
  },
};
