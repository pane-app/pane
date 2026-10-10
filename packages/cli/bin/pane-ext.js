#!/usr/bin/env node
// @pane-app/cli's `pane-ext` command (#219, ADR 0047): a small JavaScript
// shim, as esbuild, biome and turbo ship their tools, that picks the
// platform package for this system and runs the `pane-ext` program it
// holds, passing every argument through unchanged.
//
// The platform packages are this package's `optionalDependencies`, each
// named `@pane-app/cli-<target>` with the target ids Pane itself uses
// (`linux-x86_64`, `linux-aarch64`, `windows-x86_64`, `windows-aarch64`,
// `macos-x86_64`, `macos-aarch64`), carrying `pane-ext` and the
// componentizer a JavaScript or TypeScript package's build uses
// (`componentize-qjs-p3`, `runtime.wasm`, `libc.so`). npm installs the one
// whose `os` and `cpu` match this system, so exactly one is normally here;
// pane-build also looks for it by that name in a package's own
// `node_modules`, which is what an installed Pane's development mode
// componentizes with.
//
// When no platform package matches the system, or none was installed, this
// says so clearly and exits non-zero.

const { spawn } = require("node:child_process");
const path = require("node:path");

// process.platform + process.arch -> the pane_target::Target::id() spelling
// above: the six systems Pane ships for.
const TARGETS = {
  "linux-x64": "linux-x86_64",
  "linux-arm64": "linux-aarch64",
  "darwin-x64": "macos-x86_64",
  "darwin-arm64": "macos-aarch64",
  "win32-x64": "windows-x86_64",
  "win32-arm64": "windows-aarch64",
};

const system = `${process.platform} ${process.arch}`;
const target = TARGETS[`${process.platform}-${process.arch}`];
if (target === undefined) {
  console.error(
    "pane-ext: no build of pane-ext for " +
      system +
      ": Pane's command-line tool ships for Linux, macOS and Windows on x64 and arm64",
  );
  process.exit(1);
}

// The platform package npm installed beside this one (an optional
// dependency npm skips on other systems, so exactly one is normally there).
const name = `@pane-app/cli-${target}`;
let folder;
try {
  folder = path.dirname(require.resolve(`${name}/package.json`));
} catch {
  console.error(
    "pane-ext: the platform package " + name + " for " + system +
      " is not installed, so pane-ext cannot run",
  );
  console.error(
    "pane-ext: install @pane-app/cli again (`npm install @pane-app/cli --force`" +
      " reinstalls its optional dependencies, which npm sometimes skips after a failed install)",
  );
  process.exit(1);
}

const program = path.join(
  folder,
  process.platform === "win32" ? "pane-ext.exe" : "pane-ext",
);
const paneExt = spawn(program, process.argv.slice(2), { stdio: "inherit" });
paneExt.on("error", (error) => {
  console.error(`pane-ext: ${program} could not run: ${error.message}`);
  process.exit(1);
});
paneExt.on("close", (code, signal) => {
  process.exit(code ?? (signal ? 1 : 0));
});
