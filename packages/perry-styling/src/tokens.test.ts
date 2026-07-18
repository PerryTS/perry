// tokens.test.ts — regression test for the `borderWidth` design token.
//
// Perry's compiler doesn't (yet) ship a test runner for this package, so this
// follows the same pattern used across the repo's test-files/*.ts gap tests:
// a plain .ts program that throws (non-zero exit) on failure and prints a
// PASS line + exits 0 on success.
//
// Run:
//   perry compile src/tokens.test.ts -o dist/tokens-test && ./dist/tokens-test
// or:
//   npm test   (see package.json)

import { parseTokens } from "./tokens";
import { generateTheme } from "./generator";

function assertEqual(actual: number, expected: number, label: string): void {
  if (actual !== expected) {
    throw new Error(label + ": expected " + String(expected) + ", got " + String(actual));
  }
}

function assertContains(haystack: string, needle: string, label: string): void {
  if (haystack.indexOf(needle) === -1) {
    throw new Error(label + ": expected output to contain " + JSON.stringify(needle) + "\n---\n" + haystack);
  }
}

const tokensJson = JSON.stringify({
  colors: {
    primary: "#3B82F6",
  },
  spacing: { md: 16 },
  radius: { md: 8 },
  fontSize: { md: 16 },
  borderWidth: { none: 0, thin: 0.5, md: 2, lg: 4 },
});

// 1. parseTokens() must read the borderWidth section into TokenSet.borderWidth.
const tokens = parseTokens(tokensJson);

assertEqual(Object.keys(tokens.borderWidth).length, 4, "borderWidth key count");
assertEqual(tokens.borderWidth["none"], 0, "borderWidth.none");
assertEqual(tokens.borderWidth["thin"], 0.5, "borderWidth.thin");
assertEqual(tokens.borderWidth["md"], 2, "borderWidth.md");
assertEqual(tokens.borderWidth["lg"], 4, "borderWidth.lg");

// 2. generateTheme() must emit borderWidth in both the PerryTheme interface
//    and the emitted `theme` object literal — matching the documented
//    PerryTheme/ResolvedTheme shape in docs/src/ui/theming.md.
const output = generateTheme(tokens, {}, {});

assertContains(output, "borderWidth: { none: number; thin: number; md: number; lg: number; };", "PerryTheme.borderWidth field type");
assertContains(output, "borderWidth: { none: 0, thin: 0.5, md: 2, lg: 4 },", "theme.borderWidth value");

console.log("PASS: borderWidth flows through parseTokens() -> generateTheme()");
