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
      // This project runs no React Compiler (no babel-plugin-react-compiler anywhere in the
      // toolchain — see vite.config.ts), so the "recommended" preset's compiler-diagnostics
      // rule about @tanstack/react-virtual's useVirtualizer() not being safely memoizable by
      // a compiler that never runs here is not applicable; it would otherwise fire on every
      // use of the virtual list (src/components/DownloadList.tsx).
      "react-hooks/incompatible-library": "off",
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
