// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Pane's programs sample in TypeScript: a command that runs programs
// installed on the system with `@pane/extension/programs` (ADR 0033). Its
// program is `pane-echo` (guests/helpers/echo), named by its bare name, so
// Pane finds it on the user's search path at the time of the call. Items,
// answers and errors match the Rust sample (guests/sample-programs) and the
// JavaScript one.
//
// "Give up after a second" races a run against a one-second timer. A
// promise cannot be cancelled, so the run is left behind when the timer
// wins; Pane ends its program, and the one that program started, as soon
// as the call that started it returns.
import type { Command, Item } from "@pane/extension";
import { showToast } from "@pane/extension/feedback";
import {
  cmd,
  run,
  sh,
  spawn,
  type InputOptions,
  type Output,
  type ProgramError,
} from "@pane/extension/programs";
import { set } from "pane:extension/settings@0.1.0";
import { waitFor } from "wasi:clocks/monotonic-clock@0.3.0";

/** The program every item runs, by its bare name. */
const ECHO = "pane-echo";
/** The settings key where "Run until stopped" notes how far it got. */
const LONG = "programs-long";
/** How long "Give up after a second" lets its program run, in nanoseconds. */
const LIMIT = 1_000_000_000;
/** The arguments "Run by its path" passes, which no shell reads. */
const ARGS = ["--args", "two words", '"quoted"', "$HOME", "a\\b", "*", ""];

/** An exit code as the answers say it. */
const code = (exit: number | null): string => (exit === null ? "none" : String(exit));

/** Runs `pane-echo` with `args`. */
const echo = (args: string[], options: InputOptions = {}): Promise<Output> => run(ECHO, args, options);

/** The kind of a program's error, if it is one. */
const kindOf = (error: unknown): string | undefined =>
  error !== null && typeof error === "object" && "kind" in error ? String(error.kind) : undefined;

/** The absolute path `pane-echo` was found at. */
async function echoPath(): Promise<string> {
  const found = await echo(["--where"]);
  return found.stdoutText().trim();
}

/**
 * Spawns `pane-echo --lines 3`, shows each line it writes in a toast in
 * progress, then waits for it.
 */
async function stream(): Promise<string> {
  const toast = showToast({ style: "animated", title: "Starting…" });
  const process = await spawn(ECHO, ["--lines", "3"]);
  const seen: string[] = [];
  for await (const line of process.lines()) {
    toast.update({ style: "animated", title: line });
    seen.push(line);
  }
  const exit = await process.wait();
  return `Streamed ${seen.join(", ")}; exit code ${code(exit)}`;
}

/** Runs the action of the item `itemId`, showing a toast with what it did. */
async function act(itemId: string): Promise<void> {
  showToast({ title: await outcome(itemId) });
}

/** What the item `itemId` does, resolving with the text its toast shows. */
async function outcome(itemId: string): Promise<string> {
  switch (itemId) {
    case "run": {
      const output = await echo(["--status", "3"], { input: "hello" });
      return `Exit code ${code(output.exitCode)}, output "${output.stdoutText().trim()}", errors "${output.stderrText().trim()}"`;
    }
    case "path": {
      const path = await echoPath();
      const output = await run(path, ARGS);
      const arrived = output
        .stdoutText()
        .split("\n")
        .filter((line, index, all) => index < all.length - 1 || line !== "");
      return `By its path, the arguments arrived as ${arrived.map((line) => line.replace(/\r$/, "")).join(" ")}`;
    }
    case "stream":
      return stream();
    case "input": {
      const process = await spawn(ECHO, ["--echo-lines"]);
      await process.write("one\n");
      await process.write("two\n");
      await process.closeInput();
      const answers: string[] = [];
      for await (const line of process.lines()) answers.push(line);
      const exit = await process.wait();
      return `Answered ${answers.join(", ")}; exit code ${code(exit)}`;
    }
    case "kill": {
      const process = await spawn(ECHO, ["--hold", "30", "killed"]);
      await process.kill();
      await process.wait();
      return "Killed the program";
    }
    case "timeout": {
      try {
        const output = await echo(["--hold", "30", "timeout"], { timeoutMs: 500 });
        return `It finished first, with exit code ${code(output.exitCode)}`;
      } catch (error) {
        if (kindOf(error) !== "timed-out") throw error;
        return `The timeout ended it: ${(error as ProgramError).reason}`;
      }
    }
    case "descendant": {
      const process = await spawn(ECHO, ["--parent", "30"]);
      const first = await process.lines().next();
      // Returning ends the call, and with it the program and its
      // descendant.
      return `It said "${first.done ? "" : first.value}"; returned without waiting`;
    }
    case "give-up": {
      const slow = echo(["--parent", "30"]).then(
        (output) => `It finished first, with exit code ${code(output.exitCode)}`,
      );
      const timer = waitFor(LIMIT).then(() => null);
      const answer = await Promise.race([slow, timer]);
      // Returning ends the call, and with it the program and its
      // descendant.
      return answer ?? "Gave up after a second";
    }
    case "long": {
      set(LONG, "started");
      // If Pane stops the call meanwhile, the program and its descendant
      // end and nothing after this line runs.
      const output = await echo(["--parent", "60"]);
      set(LONG, "finished");
      return `Ran to the end, with exit code ${code(output.exitCode)}`;
    }
    case "flood": {
      const output = await echo(["--flood-mib", "17"]);
      return `It wrote ${output.stdout.length} bytes`;
    }
    case "elevated": {
      const output = await echo(["--where"], { elevated: true });
      return `The elevated run ended with exit code ${code(output.exitCode)}`;
    }
    case "missing": {
      const output = await run("pane-no-such-program", []);
      return `It ran, with exit code ${code(output.exitCode)}`;
    }
    case "script": {
      const script = "echo script ran";
      let output: Output;
      try {
        output = await sh(script);
      } catch (error) {
        if (kindOf(error) !== "not-found") throw error;
        output = await cmd(script);
      }
      return `The script said ${output.stdoutText().trim()}`;
    }
    case "context": {
      const path = await echoPath();
      const end = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
      if (end < 0) throw new Error(`${path} is in no folder`);
      const output = await echo(["--context", "PANE_SAMPLE_VALUE"], {
        folder: path.slice(0, end),
        env: { PANE_SAMPLE_VALUE: "set by the sample" },
      });
      return output.stdoutText().trim();
    }
    default:
      throw new Error(`unknown item: ${itemId}`);
  }
}

/** An item whose action is `act` with its id. */
const item = (id: string, title: string, subtitle: string): Item => ({
  id,
  title,
  subtitle,
  onAction: () => act(id),
});

export const command: Command = {
  async render() {
    return {
      title: "TypeScript programs sample",
      items: [
        item("run", "Run a program", "Answers its exit code, output and errors"),
        item("path", "Run by its path", "Arguments reach the program as they are"),
        item("stream", "Stream progress", "Shows each line the program writes as it comes"),
        item("input", "Talk to a program", "Writes to its input and reads its answers"),
        item("kill", "Kill a program", "Ends a program that would run on"),
        item("timeout", "Run with a timeout", "Ends the program after half a second"),
        item("descendant", "Start a descendant", "Returns while the program and the one it started run"),
        item("give-up", "Give up after a second", "Drops the run when a timer wins"),
        item("long", "Run until stopped", "Runs for a minute; disabling or reloading ends it"),
        item("flood", "Write too much", "The program writes more than Pane keeps"),
        item("elevated", "Run elevated", "Asks Windows to run it as an administrator"),
        item("missing", "Run a missing program", "Names a program that is on no search path"),
        item("script", "Run a shell script", "Runs echo through the system's shell"),
        item("context", "Run in a folder with a variable", "Its own folder, and a variable set"),
      ],
    };
  },

  async run(commandId) {
    if (commandId !== "progress") throw new Error(`\`${commandId}\` opens a screen`);
    showToast({ title: await stream() });
  },
};
