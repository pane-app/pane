// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Package search, the online search sample, in TypeScript: a command that
// searches a web service as the user types into its own search field. Pane
// asks it only once the user has opened it, never while they type in root
// search. The service is the fixture service, a made-up package registry on
// this computer (`cargo run -p pane-core --example fixture_service`, port
// 8740 by default); the command reaches it with `get` from
// `@pane/extension/http` (`wasi:http` underneath) and its address is a
// setting the command's form changes. A search Pane no longer needs is
// stopped where it waits; an unreachable or failing service is an error
// shown in place of results, not a crash. Items, toasts and errors match
// the Rust sample (guests/sample-search) and the JavaScript one.
import type { Command, CommandSearch, FieldValue, Form, SearchResult } from "@pane/extension";
import { showToast } from "@pane/extension/feedback";
import { get as fetchUrl } from "@pane/extension/http";
import { get, set } from "pane:extension/settings@0.1.0";

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

/** The service address: the saved one, or the default. */
function service(): string {
  return host(() => get(SERVICE)) ?? DEFAULT_SERVICE;
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

const SERVICE_FORM: Form = {
  title: "Service address",
  fields: [{ id: "address", label: "Address", kind: { tag: "text", val: { placeholder: DEFAULT_SERVICE } } }],
  submitLabel: "Save",
};

/**
 * Runs the action `itemId`: the "about" item's, or a search result's
 * ("package:<name>"), which fetches that package's details; it shows a
 * toast with what it found.
 */
async function act(itemId: string): Promise<void> {
  showToast({ title: await outcome(itemId) });
}

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

export const command: Command = {
  async render() {
    return {
      title: "Package search",
      items: [
        {
          id: "about",
          title: "Type to search the package registry",
          subtitle: "Results come from the service as you type; Enter shows a package's details",
          onAction: () => act("about"),
        },
        { id: "service", title: "Service address", subtitle: service(), form: SERVICE_FORM },
      ],
    };
  },
  // A search result's id ("package:<name>") names the package to show.
  async runSearchResult(id: string) {
    await act(id);
  },
  async submitForm(itemId: string, values: FieldValue[]) {
    if (itemId !== "service") {
      throw { message: `unknown form: ${itemId}` };
    }
    const address = (values.find(({ id }) => id === "address")?.value ?? "").trim().replace(/\/+$/, "");
    if (!(address.startsWith("http://") || address.startsWith("https://"))) {
      throw { field: "address", message: "Enter an address starting with http:// or https://" };
    }
    host(() => set(SERVICE, address));
    return `Searching ${address} from now on`;
  },
  async openView() {
    throw new Error("Package search has no custom views");
  },
};

export const commandSearch: CommandSearch = {
  async search(command: string, query: string): Promise<SearchResult[]> {
    if (command !== COMMAND) throw new Error(`unknown command: ${command}`);
    const found = await fetchJson<Found>(`/search?q=${encodeURIComponent(query)}`);
    return found.results.map((pkg) => ({
      id: `package:${pkg.name}`,
      title: pkg.name,
      subtitle: pkg.summary,
    }));
  },
};
