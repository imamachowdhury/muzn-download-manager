import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";

export default tseslint.config(
  { ignores: ["dist", "target", "src-tauri", "crates", "node_modules"] },
  {
    files: ["**/*.{ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    languageOptions: { globals: globals.browser },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "no-restricted-globals": ["error", "confirm", "alert", "prompt"],
      "no-restricted-properties": [
        "error",
        { object: "window", property: "confirm" },
        { object: "window", property: "alert" },
        { object: "window", property: "prompt" },
      ],
    },
  },
);
