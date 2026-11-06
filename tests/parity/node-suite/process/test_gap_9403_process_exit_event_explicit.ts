// #9403 — `process.on("exit", …)` on the EXPLICIT `process.exit(n)` path.
//
// Node: `process.exit(3)` emits `exit` with 3 and never emits `beforeExit`
// (that hook fires only when the loop drains on its own). Nothing scheduled
// from inside a handler runs — not even a promise job, unlike the natural
// drain, because `process.exit` terminates without returning to a microtask
// checkpoint.
//
// Also pins listener bookkeeping for this event: `once`, `prependListener`
// and `removeListener` must all be honoured by whatever emits `exit`.
//
// Companion expected-exit file asserts the status is 3.
process.on("beforeExit", () => {
  console.log("BUG: beforeExit fired on an explicit process.exit()");
});

process.on("exit", (code: number) => {
  console.log("A code=" + code);
});

process.once("exit", (code: number) => {
  console.log("B-once code=" + code);
});

const removed = (code: number) => {
  console.log("BUG: removed listener ran, code=" + code);
};
process.on("exit", removed);
process.removeListener("exit", removed);

process.prependListener("exit", (code: number) => {
  console.log("C-prepended code=" + code);
  setTimeout(() => console.log("BUG: setTimeout callback ran"), 0);
  Promise.resolve().then(() => console.log("BUG: promise job ran"));
});

console.log("listenerCount=" + process.listenerCount("exit"));
console.log("main done");
process.exit(3);
