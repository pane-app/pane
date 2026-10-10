// The package's own linter, which `pane-ext check` runs through npm: the
// recommended rules for JavaScript and TypeScript. `_`-prefixed parameters
// are allowed to go unused: the entry points a template declares keep
// their parameters for the commands `pane-ext new command` adds.
import js from "@eslint/js";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    rules: {
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_" }],
    },
  },
);
