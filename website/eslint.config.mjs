import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  // Override default ignores of eslint-config-next.
  globalIgnores([
    // Default ignores of eslint-config-next:
    ".next/**",
    "out/**",
    "build/**",
    "next-env.d.ts",
    // Bklit UI registry source is third-party code. Our adapter and use sites remain linted.
    "src/components/charts/**",
    // Experimental chart primitives are kept as optional UI inventory, but
    // the static export currently ships a lighter benchmark component.
    "src/components/charts/**",
    "src/components/kokonutui/**",
  ]),
]);

export default eslintConfig;
