// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `@pane/extension/programs` (programs.js): programs
// installed on the system, which Pane runs for the command, through
// `pane:extension/programs@0.1.0` (programs-host.d.ts, wit/programs.wit).

/**
 * Why a program did not run or answer. `kind` tells the cases apart:
 *
 * - `not-found`: no program by that name on the search path, no file at
 *   that path, or no folder at `folder`;
 * - `unavailable`: it cannot run so here (an elevated run outside Windows,
 *   an elevated `spawn`, a file the system would not start);
 * - `declined`: the user declined Windows' elevation prompt;
 * - `timed-out`: `timeoutMs` passed, and Pane ended it with every process
 *   it started;
 * - `too-much-output`: a `run`'s program wrote more than Pane keeps (16 MiB
 *   of a stream), and Pane ended it;
 * - `refused`: Pane refused or ended the run (the extension's code was
 *   stopped, Pane is quitting, the call that started it ended, its input
 *   was closed, a relative path, over Pane's limits);
 * - `failed`: Pane lost track of the program or of one of its streams.
 *
 * `reason` says why for people; the message is `<kind>: <reason>`.
 */
export class ProgramError extends Error {
  readonly kind:
    | "not-found"
    | "unavailable"
    | "declined"
    | "timed-out"
    | "too-much-output"
    | "refused"
    | "failed";
  readonly reason: string;
}

/** How and where a program runs. */
export interface RunOptions {
  /** The absolute path of the folder it runs in; the user's home folder when omitted. */
  folder?: string | null;
  /** Variables to set for it, or with `null` to remove, after Pane's own. */
  env?: Record<string, string | null>;
  /** Ends it after this many milliseconds, rejecting with `timed-out`. */
  timeoutMs?: number | null;
  /** Gives it a console window of its own on Windows. */
  showWindow?: boolean;
  /**
   * Runs it as an administrator, through Windows' elevation prompt: the
   * run then resolves with its exit code only, and takes no input or `env`.
   * Elsewhere it rejects with `unavailable` for now.
   */
  elevated?: boolean;
}

/** `run`'s options: those of {@link RunOptions}, and its input. */
export interface InputOptions extends RunOptions {
  /** Written to its standard input, which is then closed. */
  input?: string | Uint8Array | null;
}

/** How a program run with `run` exited, and what it wrote. */
export interface Output {
  /** Its exit code; `null` when the system ended it. */
  readonly exitCode: number | null;
  readonly stdout: Uint8Array;
  readonly stderr: Uint8Array;
  /** Whether it exited with code 0. */
  readonly success: boolean;
  /** Its standard output as text, invalid UTF-8 replaced. */
  stdoutText(): string;
  /** Its standard error as text, invalid UTF-8 replaced. */
  stderrText(): string;
}

/** A program `spawn` started, which ends with the call that started it. */
export interface Process {
  /** Pane's id for it. */
  readonly id: bigint;
  /** Writes `data` to its standard input. */
  write(data: string | Uint8Array): Promise<void>;
  /** Closes its standard input. */
  closeInput(): Promise<void>;
  /** The next bytes it wrote to its standard output; `null` once it ended. */
  read(): Promise<Uint8Array | null>;
  /** The next bytes it wrote to its standard error; `null` once it ended. */
  readError(): Promise<Uint8Array | null>;
  /** Waits for it to exit: its exit code, `null` when the system ended it. */
  wait(): Promise<number | null>;
  /** Ends it and every process it started; `wait` resolves with how it exited. */
  kill(): Promise<void>;
  /** Its standard output, line by line as it writes it, without line endings. */
  lines(): AsyncIterableIterator<string>;
}

/**
 * Runs `program` (an absolute path, or a bare name found on the user's
 * search path now) with `args`, each reaching it as it is, and resolves
 * with how it exited and what it wrote, whatever its exit code.
 */
export function run(program: string, args?: string[], options?: InputOptions): Promise<Output>;

/** Starts `program` with `args`, resolving with the process to work with. */
export function spawn(program: string, args?: string[], options?: RunOptions): Promise<Process>;

/** Runs `script` with Windows PowerShell, passed encoded so that no quoting changes it. */
export function powershell(script: string, options?: RunOptions): Promise<Output>;

/**
 * Runs `script` with Windows' command interpreter (`cmd /d /s /c`); quotes
 * in it may not survive Windows' quoting of arguments.
 */
export function cmd(script: string, options?: RunOptions): Promise<Output>;

/** Runs `script` with the system's shell (`sh -c`), on macOS and Linux. */
export function sh(script: string, options?: RunOptions): Promise<Output>;

/** UTF-8 `bytes` as text, invalid sequences replaced with U+FFFD. */
export function text(bytes: Uint8Array): string;
