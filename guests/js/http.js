// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Web requests for JS/TS commands (`@pane/extension/http`), through
// `wasi:http@0.3.0`'s client, which Pane sends for the command (`http` and
// `https`, over HTTP/1.1, trusting the system's certificates). Bundled into
// the command that imports it, like any npm module.
//
// `get` covers the common case: fetch an address and read the whole
// response. For anything else (other methods, request bodies, reading a
// body as it arrives), import `wasi:http/types@0.3.0` and
// `wasi:http/client@0.3.0` directly, as this module does.
//
// A request runs inside the call that made it: when Pane stops that call (a
// search the user replaced by typing on, a command left, a package disabled
// or reloaded), the request is dropped with it and nothing after its
// `await` runs.

import { send } from "wasi:http/client@0.3.0";
import { Fields, Request, Response } from "wasi:http/types@0.3.0";

/** `text` as UTF-8 bytes. */
function encodeUtf8(text) {
  const bytes = [];
  for (const character of text) {
    let code = character.codePointAt(0);
    if (code < 0x80) {
      bytes.push(code);
    } else if (code < 0x800) {
      bytes.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
    } else if (code < 0x10000) {
      bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
    } else {
      bytes.push(
        0xf0 | (code >> 18),
        0x80 | ((code >> 12) & 0x3f),
        0x80 | ((code >> 6) & 0x3f),
        0x80 | (code & 0x3f),
      );
    }
  }
  return new Uint8Array(bytes);
}

/** UTF-8 `bytes` as text, invalid sequences replaced with U+FFFD. */
function decodeUtf8(bytes) {
  let text = "";
  let index = 0;
  while (index < bytes.length) {
    const first = bytes[index];
    const length = first < 0x80 ? 1 : first >= 0xf0 ? 4 : first >= 0xe0 ? 3 : first >= 0xc0 ? 2 : 0;
    let code = length === 1 ? first : length === 2 ? first & 0x1f : length === 3 ? first & 0x0f : first & 0x07;
    let valid = length > 0 && index + length <= bytes.length;
    for (let next = 1; valid && next < length; next += 1) {
      const byte = bytes[index + next];
      valid = (byte & 0xc0) === 0x80;
      code = (code << 6) | (byte & 0x3f);
    }
    if (valid) {
      text += String.fromCodePoint(code);
      index += length;
    } else {
      text += "�";
      index += 1;
    }
  }
  return text;
}

/** Why no response came, in words, from a `wasi:http` error code. */
export function explain(error) {
  const code = error !== null && typeof error === "object" && "payload" in error ? error.payload : error;
  const tag = code !== null && typeof code === "object" ? code.tag : String(code);
  switch (tag) {
    case "DNS-timeout":
      return "looking up the address timed out";
    case "DNS-error":
      return "the address could not be resolved";
    case "destination-not-found":
      return "the host was not found";
    case "destination-unavailable":
      return "the host is unavailable";
    case "destination-IP-unroutable":
      return "the host cannot be reached";
    case "connection-refused":
      return "connection refused";
    case "connection-terminated":
      return "the connection was closed";
    case "connection-timeout":
      return "connecting timed out";
    case "connection-read-timeout":
      return "the service did not answer in time";
    case "HTTP-response-timeout":
      return "the service took too long to answer";
    case "HTTP-response-body-size":
      return typeof code.val === "bigint" || typeof code.val === "number"
        ? `the answer is larger than the ${code.val} bytes Pane accepts`
        : "the answer is too large";
    case "connection-limit-reached":
      return "too many of the extension's requests are open at once";
    case "TLS-certificate-error":
      return "the host's certificate is not trusted";
    case "TLS-protocol-error":
    case "TLS-alert-received":
      return "the secure connection failed";
    case "HTTP-request-denied":
      return "Pane did not send the request";
    case "internal-error":
      return typeof code.val === "string" ? code.val : "internal-error";
    default:
      return tag;
  }
}

/** `url`'s scheme, authority and path with its query. */
function split(url) {
  const match = /^(https?):\/\/([^/?#]+)([^#]*)/.exec(url);
  if (match === null) throw new Error(`${url} is not an http:// or https:// address`);
  const [, scheme, authority, path] = match;
  return { scheme: scheme === "https" ? { tag: "HTTPS" } : { tag: "HTTP" }, authority, path: path || "/" };
}

/**
 * Fetches `url`, an absolute `http://` or `https://` address, with GET and
 * the extra `headers` (an object of names and values), and reads the whole
 * response. Any status is a response; a failure to get one throws an
 * `Error` whose message says why, such as "connection refused".
 *
 * @param {string} url
 * @param {Record<string, string>} [headers]
 * @returns {Promise<import("./http").HttpResponse>}
 */
export async function get(url, headers = {}) {
  const { scheme, authority, path } = split(url);
  const fields = new Fields();
  for (const [name, value] of Object.entries(headers)) {
    fields.append(name, encodeUtf8(value));
  }
  // No body and no trailers.
  const trailers = wit.Future.from({ tag: "ok", val: null }, wit.Future.RESULT_OPTION_OTHER_ERROR_CODE);
  const [request] = Request.new(fields, null, trailers.readable, null);
  try {
    request.setScheme(scheme);
    request.setAuthority(authority);
    request.setPathWithQuery(path);
  } catch {
    throw new Error(`${url} is not a web address Pane can send`);
  }
  let response;
  try {
    response = await send(request);
  } catch (error) {
    throw new Error(explain(error));
  }
  const status = response.getStatusCode();
  const received = response
    .getHeaders()
    .copyAll()
    .map(([name, value]) => [name.toLowerCase(), decodeUtf8(value)]);
  const read = wit.Future.from({ tag: "ok", val: undefined }, wit.Future.RESULT_VOID_ERROR_CODE);
  const [body, ended] = Response.consumeBody(response, read.readable);
  const chunks = [];
  let length = 0;
  for await (const chunk of body) {
    chunks.push(chunk);
    length += chunk.length;
  }
  // Whether the body arrived whole: a connection that broke off midway is
  // an error, not a shorter body.
  const end = await ended.read();
  if (end !== null && typeof end === "object" && end.tag === "err") {
    throw new Error(explain(end.val));
  }
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return {
    status,
    headers: received,
    body: bytes,
    header(name) {
      const found = received.find(([key]) => key === name.toLowerCase());
      return found === undefined ? undefined : found[1];
    },
    text() {
      return decodeUtf8(bytes);
    },
    json() {
      return JSON.parse(decodeUtf8(bytes));
    },
  };
}
