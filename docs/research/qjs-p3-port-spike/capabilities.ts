import { query as search } from "../wasi03-js-spike/search.ts";
import { getDirectories } from "wasi:filesystem/preopens@0.3.0";

let invocations = 0;

export async function query(text: string): Promise<string> {
  const result = JSON.parse(await search("calclator"));
  const directories = getDirectories();
  let fileText = "";
  let fileError: string | null = null;
  let fileErrorPayload: unknown = null;
  let completion: unknown = null;
  try {
    if (!directories.length) throw new Error("no-preopens");
    const descriptor = await directories[0][0].openAt(
      {}, text === "missing" ? "missing.txt" : "fixture.txt", {}, { read: true },
    );
    try {
      const [stream, finished] = descriptor.readViaStream(0);
      try {
        for await (const chunk of stream) {
          for (const byte of (typeof chunk === "number" ? [chunk] : chunk)) {
            fileText += String.fromCharCode(byte); // ASCII fixture, not a general UTF-8 decoder.
          }
        }
        completion = await finished.read();
      } finally {
        stream.drop();
        finished.drop();
      }
    } finally {
      descriptor[Symbol.dispose]();
    }
  } catch (error) {
    fileError = String(error);
    fileErrorPayload = error?.payload ?? null;
  } finally {
    for (const [descriptor] of directories) descriptor[Symbol.dispose]();
  }
  return JSON.stringify({ ...result, invocations: ++invocations,
    dateMs: Date.now(), random: Math.random(), fileText, fileError, fileErrorPayload, completion });
}
