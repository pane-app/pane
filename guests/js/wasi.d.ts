// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Declarations for the WASI 0.3 imports that wit/world.wit declares. WIT u64
// values are JavaScript numbers.

/** `wasi:clocks/monotonic-clock@0.3.0`; durations are in nanoseconds. */
declare module "wasi:clocks/monotonic-clock@0.3.0" {
  /** The current reading of the monotonic clock. */
  export function now(): number;
  /** The clock's resolution. */
  export function getResolution(): number;
  /** Resolves once the clock reaches `when`; the command suspends meanwhile. */
  export function waitUntil(when: number): Promise<void>;
  /** Resolves after `howLong` has elapsed; the command suspends meanwhile. */
  export function waitFor(howLong: number): Promise<void>;
}
