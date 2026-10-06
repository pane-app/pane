// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// System programs for JS/TS commands (`@pane/extension/programs`), through
// `pane:extension/programs@0.1.0` (wit/programs.wit): programs installed on
// the system, such as PowerShell, winget, git or any executable, which Pane
// runs for the command. Bundled into the command that imports it, like any
// npm module; only a command whose bundle uses it imports the interface,
// and Pane lists such a command's package as one that runs system programs.
//
// `run` waits for a program and resolves with its exit code and what it
// wrote (bytes, with text helpers); `spawn` resolves with a process the
// command writes to, reads from as it writes (to report progress in a
// toast), waits for and kills. A program is named by an absolute path, or
// by a bare name Pane finds on the user's search path at the time of the
// call; each argument reaches it as it is, since no shell reads them.
// `powershell`, `cmd` and `sh` run a script with the system's shells.
//
// A program belongs to the call that started it: Pane ends it, and every
// process it started, when the call returns, when the extension is
// disabled, reloaded, updated or uninstalled, and when Pane quits. A
// promise cannot be cancelled, so a run left behind (when a timer wins a
// race with it) ends once the call returns.

import {
  closeInput,
  kill as killProcess,
  readError,
  readOutput,
  run as runProgram,
  spawn as spawnProgram,
  wait as waitFor,
  writeInput,
} from "pane:extension/programs@0.1.0";

/**
 * Why a program did not run or answer: `kind` tells the cases apart
 * (`"not-found"`, `"timed-out"`, ...), `reason` says why for people, and
 * the error's message is `<kind>: <reason>`.
 */
export class ProgramError extends Error {
  constructor(kind, reason) {
    super(`${kind}: ${reason}`);
    this.name = "ProgramError";
    this.kind = kind;
    this.reason = reason;
  }
}

/** What a rejected call into Pane threw, as a `ProgramError` where it is one. */
function programError(error) {
  const payload =
    error !== null && typeof error === "object" && "payload" in error ? error.payload : null;
  if (payload !== null && typeof payload === "object" && typeof payload.kind === "string") {
    return new ProgramError(payload.kind, String(payload.message));
  }
  return error;
}

/** Calls Pane with `call`, rejecting with a `ProgramError` where Pane answers one. */
async function call(start) {
  try {
    return await start();
  } catch (error) {
    throw programError(error);
  }
}

/** `text` as UTF-8 bytes. */
function encodeUtf8(text) {
  const bytes = [];
  for (const character of text) {
    const code = character.codePointAt(0);
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
export function text(bytes) {
  let decoded = "";
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
      decoded += String.fromCodePoint(code);
      index += length;
    } else {
      decoded += "�";
      index += 1;
    }
  }
  return decoded;
}

/** `data` (text or bytes) as bytes. */
function bytes(data) {
  if (data == null) return new Uint8Array(0);
  if (data instanceof Uint8Array) return data;
  return encodeUtf8(String(data));
}

/** `options` as the WIT carries them. */
function wire(options) {
  const environment = Object.entries(options?.env ?? {}).map(([name, value]) => ({
    name,
    value: value == null ? null : String(value),
  }));
  return {
    folder: options?.folder == null ? null : String(options.folder),
    environment,
    timeoutMs: options?.timeoutMs == null ? null : Math.max(0, Math.floor(Number(options.timeoutMs))),
    showWindow: Boolean(options?.showWindow),
    elevated: Boolean(options?.elevated),
  };
}

/** How a program run with `run` exited, and what it wrote. */
class Output {
  constructor(output) {
    this.exitCode = output.exitCode ?? null;
    this.stdout = output.stdout instanceof Uint8Array ? output.stdout : new Uint8Array(output.stdout ?? []);
    this.stderr = output.stderr instanceof Uint8Array ? output.stderr : new Uint8Array(output.stderr ?? []);
  }

  /** Whether it exited with code 0. */
  get success() {
    return this.exitCode === 0;
  }

  /** What it wrote to its standard output, as text. */
  stdoutText() {
    return text(this.stdout);
  }

  /** What it wrote to its standard error, as text. */
  stderrText() {
    return text(this.stderr);
  }
}

/**
 * Runs `program` with `args` and `options.input` on its standard input,
 * and resolves with how it exited and what it wrote, whatever its exit
 * code.
 */
export async function run(program, args = [], options = {}) {
  const output = await call(() =>
    runProgram(String(program), args.map(String), bytes(options?.input), wire(options)),
  );
  return new Output(output);
}

/** A program `spawn` started, which ends with the call that started it. */
class Process {
  constructor(id) {
    this.id = id;
  }

  /** Writes `data` (text or bytes) to its standard input. */
  write(data) {
    return call(() => writeInput(this.id, bytes(data)));
  }

  /** Closes its standard input. */
  closeInput() {
    return call(() => closeInput(this.id));
  }

  /** The next bytes it wrote to its standard output; `null` once it ended. */
  async read() {
    const chunk = await call(() => readOutput(this.id));
    return chunk == null ? null : chunk;
  }

  /** The next bytes it wrote to its standard error; `null` once it ended. */
  async readError() {
    const chunk = await call(() => readError(this.id));
    return chunk == null ? null : chunk;
  }

  /** Waits for it to exit: its exit code, `null` when the system ended it. */
  async wait() {
    const code = await call(() => waitFor(this.id));
    return code == null ? null : code;
  }

  /** Ends it and every process it started; `wait` resolves with how it exited. */
  kill() {
    return call(() => killProcess(this.id));
  }

  /** Its standard output, line by line as it writes it, without line endings. */
  async *lines() {
    let pending = new Uint8Array(0);
    for (;;) {
      const chunk = await this.read();
      if (chunk === null) break;
      const joined = new Uint8Array(pending.length + chunk.length);
      joined.set(pending, 0);
      joined.set(chunk, pending.length);
      pending = joined;
      let end = pending.indexOf(10);
      while (end !== -1) {
        yield text(pending.subarray(0, end)).replace(/\r$/, "");
        pending = pending.slice(end + 1);
        end = pending.indexOf(10);
      }
    }
    if (pending.length > 0) yield text(pending);
  }
}

/** Starts `program` with `args`, resolving with the process to work with. */
export async function spawn(program, args = [], options = {}) {
  const id = await call(() => spawnProgram(String(program), args.map(String), wire(options)));
  return new Process(id);
}

/** `bytes` in standard base64, padded. */
function base64(data) {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let encoded = "";
  for (let index = 0; index < data.length; index += 3) {
    const count = Math.min(3, data.length - index);
    const triple = (data[index] << 16) | ((data[index + 1] ?? 0) << 8) | (data[index + 2] ?? 0);
    for (let place = 0; place < 4; place += 1) {
      encoded += place <= count ? alphabet[(triple >> (18 - 6 * place)) & 0x3f] : "=";
    }
  }
  return encoded;
}

/** `script` as PowerShell's `-EncodedCommand` takes it: UTF-16LE in base64. */
function encodedCommand(script) {
  const units = [];
  for (let index = 0; index < script.length; index += 1) {
    const unit = script.charCodeAt(index);
    units.push(unit & 0xff, unit >> 8);
  }
  return base64(units);
}

/** Runs `script` with Windows PowerShell, passed encoded so that no quoting changes it. */
export function powershell(script, options = {}) {
  return run(
    "powershell",
    ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", encodedCommand(String(script))],
    options,
  );
}

/**
 * Runs `script` with Windows' command interpreter (`cmd /d /s /c`), which
 * reads the script itself: quotes in it may not survive Windows' quoting of
 * arguments, which `powershell` avoids.
 */
export function cmd(script, options = {}) {
  return run("cmd", ["/d", "/s", "/c", String(script)], options);
}

/** Runs `script` with the system's shell (`sh -c`), on macOS and Linux. */
export function sh(script, options = {}) {
  return run("sh", ["-c", String(script)], options);
}
