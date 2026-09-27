// Throwaway probe: real npm libraries and a real asynchronous WASI 0.3 import.
import Fuse from "fuse.js";
import { z } from "zod";
import { now, waitFor } from "wasi:clocks/monotonic-clock@0.3.0";

const rows = z.array(z.object({ id: z.string(), title: z.string() })).parse([
  { id: "calculator", title: "Calculator" },
  { id: "applications", title: "Applications" },
  { id: "quicklinks", title: "Quicklinks" },
]);
const index = new Fuse(rows, { keys: ["title"], threshold: 0.4 });

export async function query(text: string): Promise<string> {
  const start = now();
  // This QuickJS backend currently lowers WIT u64 from JS Number, not BigInt.
  await waitFor(10_000_000);
  const items = text ? index.search(text).map(({ item }) => item) : rows;
  return JSON.stringify({
    title: "TypeScript: WASI 0.3 + Fuse.js + Zod",
    items,
    elapsedNs: String(now() - start),
    invalidDataRejected: !z.string().safeParse(42).success,
  });
}
