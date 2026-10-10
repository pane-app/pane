#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// @pane-app/create's bin, the one `npm create @pane-app` runs: it starts
// `pane-ext new` with the arguments it was given, inheriting the terminal
// so pane-ext's prompts and its output show as if the author had run it
// directly. `npm create @pane-app notes -- --language typescript` runs
// `pane-ext new notes --language typescript`.
//
// Where pane-ext comes from: this package depends on @pane-app/cli, whose
// per-platform packages put the pane-ext binary in
// node_modules/@pane-app/cli-<target>, the same place Pane's own build
// looks for the componentizer — so npm's one-step create installs it and
// this finds it. A pane-ext on the PATH (a globally installed CLI) serves
// too. With neither, this says how to get one instead of failing obscurely.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** Pane's target id of this system, as @pane-app/cli's platform packages
 *  are named, or null where Pane does not run. */
function target() {
  const platform =
    process.platform === "win32"
      ? "windows"
      : process.platform === "darwin"
        ? "macos"
        : process.platform === "linux"
          ? "linux"
          : null;
  const arch =
    process.arch === "x64" ? "x86_64" : process.arch === "arm64" ? "aarch64" : null;
  return platform && arch ? `${platform}-${arch}` : null;
}

/** The pane-ext binary an installed @pane-app/cli holds for this system,
 *  walking the node_modules folders above this package, or null. */
function installed() {
  const target = target();
  if (target === null) return null;
  const program = process.platform === "win32" ? "pane-ext.exe" : "pane-ext";
  let folder = path.dirname(fileURLToPath(import.meta.url));
  while (true) {
    const beside = path.join(folder, "node_modules", `@pane-app/cli-${target}`, program);
    if (existsSync(beside)) return beside;
    const parent = path.dirname(folder);
    if (parent === folder) return null;
    folder = parent;
  }
}

const paneExt = installed() ?? "pane-ext";
const run = spawnSync(paneExt, ["new", ...process.argv.slice(2)], { stdio: "inherit" });
if (run.error) {
  console.error(
    "@pane-app/create: found no pane-ext to run. `npm create @pane-app` installs it " +
      "with @pane-app/cli; if this did not, install the CLI where npm can find it: " +
      "`npm install -g @pane-app/cli`.",
  );
  process.exit(1);
}
process.exit(run.status ?? 1);
