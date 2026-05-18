// V8-fallback helper used by `test_v8_namespace_call.ts`. Lives in a
// directory with no package.json + .mjs extension so Perry's module
// resolver routes it to the deno_core V8 fallback rather than trying
// to compile it natively. Covers the three return-value shapes
// (number from array-reducing call, string, class instance with
// methods) that crossed the V8 boundary in the bug ramda's `R.sum`,
// effect's `Effect.succeed`, jose's signing chain.
export function sum(arr) { return arr.reduce((a, b) => a + b, 0); }
export function greet(name) { return 'hello ' + name; }
export class Thing {
  constructor(v) { this.value = v; }
  doubled() { return this.value * 2; }
}
export function make(v) { return new Thing(v); }
