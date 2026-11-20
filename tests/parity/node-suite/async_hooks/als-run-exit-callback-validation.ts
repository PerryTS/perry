// Issue #3092 — `AsyncLocalStorage#run(store, cb)` and `#exit(cb)` reject a
// non-callable callback with a `TypeError` (Node throws through its
// function-apply path). Assert the error name only — the V8-internal apply
// message is an implementation detail, not part of the observable contract.
import { AsyncLocalStorage } from "node:async_hooks";

const als = new AsyncLocalStorage();

function probe(method, label, fn) {
  try {
    fn();
    console.log(method, label, "no-throw");
  } catch (err) {
    console.log(method, label, err.name);
  }
}

const bad = [
  ["undefined", undefined],
  ["null", null],
  ["number", 0],
  ["boolean", true],
  ["string", "x"],
  ["object", {}],
  ["array", []],
];
for (const [label, value] of bad) {
  probe("run", label, () => als.run("store", value));
}
for (const [label, value] of bad) {
  probe("exit", label, () => als.exit(value));
}

// Issue #3093 - valid callbacks receive forwarded rest arguments while store
// visibility and exit restoration stay intact.
const runOut = als.run(
  "S",
  function (...args) {
    console.log("valid run args:", JSON.stringify(args));
    return als.getStore();
  },
  "a",
  "b",
  "c",
);
console.log("valid run store:", runOut);

als.enterWith("OUTER");
const exitOut = als.exit(
  function (...args) {
    console.log("valid exit args:", JSON.stringify(args));
    return als.getStore();
  },
  "x",
  "y",
);
console.log("valid exit store:", exitOut);
console.log("store after exit:", als.getStore());
