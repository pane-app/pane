// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `@pane-app/extension/http` (http.js): web requests through
// `wasi:http@0.3.0`'s client, which Pane sends for the command.

/** A response read to its end. */
export interface HttpResponse {
  /** The HTTP status code, such as 200. */
  status: number;
  /** The headers, names in lower case, in the order received. */
  headers: [string, string][];
  /** The body's bytes. */
  body: Uint8Array;
  /** The first value of header `name` (compared without case). */
  header(name: string): string | undefined;
  /** The body as text, invalid UTF-8 replaced with U+FFFD. */
  text(): string;
  /** The body parsed as JSON; throws if it is not JSON. */
  json(): unknown;
}

/**
 * Fetches `url`, an absolute `http://` or `https://` address, with GET and
 * the extra `headers`, and reads the whole response. Any status is a
 * response; a failure to get one throws an `Error` whose message says why,
 * such as "connection refused" or "the address could not be resolved".
 */
export function get(url: string, headers?: Record<string, string>): Promise<HttpResponse>;

/** Why no response came, in words, from a `wasi:http` error code. */
export function explain(error: unknown): string;
