// Issue #3092 — `AsyncLocalStorage#run(store, cb)` and `#exit(cb)` reject a
// non-callable callback with a `TypeError` (Node throws through its
// function-apply path). Assert the error name only — the V8-internal apply
// message is an implementation detail, not part of the observable contract.
import { AsyncLocalStorage } from "node:async_hooks";

const als = new AsyncLocalStorage<string>();

function probe(method: string, label: string, fn: () => unknown) {
  try {
    fn();
    console.log(method, label, "no-throw");
  } catch (err: any) {
    console.log(method, label, err.name);
  }
}

const bad: [string, unknown][] = [
  ["undefined", undefined],
  ["null", null],
  ["number", 0],
  ["boolean", true],
  ["string", "x"],
  ["object", {}],
  ["array", []],
];
for (const [label, value] of bad) {
  probe("run", label, () => als.run("store", value as any));
}
for (const [label, value] of bad) {
  probe("exit", label, () => als.exit(value as any));
}

// Valid callbacks still run, with the expected store visibility.
const runOut = als.run("S", () => als.getStore());
console.log("valid run store:", runOut);

als.enterWith("OUTER");
const exitOut = als.exit(() => als.getStore());
console.log("valid exit store:", exitOut);
console.log("store after exit:", als.getStore());
