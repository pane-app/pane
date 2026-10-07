// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for `pane:extension/programs` in wit/programs.wit: running
// programs installed on the system. Most commands use them through
// `@pane/extension/programs` (programs.d.ts). A command imports the
// interface only if its bundle uses it.

/** `pane:extension/programs@0.1.0`. */
declare module "pane:extension/programs@0.1.0" {
  export type ProgramErrorKind =
    | "not-found"
    | "unavailable"
    | "declined"
    | "timed-out"
    | "too-much-output"
    | "refused"
    | "failed";

  /** `message` explains the failure for people. */
  export interface ProgramError {
    kind: ProgramErrorKind;
    message: string;
  }

  /** A variable to set, or with `null` to remove. */
  export interface EnvVar {
    name: string;
    value?: string | null;
  }

  export interface Options {
    folder?: string | null;
    environment: EnvVar[];
    timeoutMs?: number | null;
    showWindow: boolean;
    elevated: boolean;
  }

  export interface Output {
    exitCode?: number | null;
    stdout: Uint8Array;
    stderr: Uint8Array;
  }

  /**
   * Each rejects with an object whose `payload` is the {@link ProgramError}
   * on failure.
   */
  export function run(
    program: string,
    args: string[],
    input: Uint8Array,
    options: Options,
  ): Promise<Output>;
  export function spawn(program: string, args: string[], options: Options): Promise<bigint>;
  export function writeInput(process: bigint, bytes: Uint8Array): Promise<void>;
  export function closeInput(process: bigint): Promise<void>;
  export function readOutput(process: bigint): Promise<Uint8Array | null>;
  export function readError(process: bigint): Promise<Uint8Array | null>;
  export function wait(process: bigint): Promise<number | null>;
  export function kill(process: bigint): Promise<void>;
}
