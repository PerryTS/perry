// Foo "module" — the body of a namespace re-export from index.ts.
// Mirrors the pattern Effect uses: index.ts does `export * as Effect
// from "./Effect.js"` and Effect.ts holds the actual implementation.

export function succeed(value: number): number {
  return value;
}

export function add(a: number, b: number): number {
  return a + b;
}

export function runSync(value: number): number {
  return value;
}

export const tag = "Foo-namespace";
