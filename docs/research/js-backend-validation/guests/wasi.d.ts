// Hand-written ambient types for the one WASI import the TS sample uses.
// componentize-qjs 0.4.5 does not generate TypeScript declarations; u64 is lowered from Number.
declare module "wasi:clocks/monotonic-clock@0.3.0" {
  export function now(): number;
  export function waitFor(howLongNs: number): Promise<void>;
}
