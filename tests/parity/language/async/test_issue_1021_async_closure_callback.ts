// Issue #1021: async closures with awaits used to busy-wait at expr.rs:10588
// and deadlock self-fetch inside V8 trampoline frames. Phase 2 routes them
// through the same state-machine + async-step-driver as top-level async fns.
//
// Minimal reproducer (no V8/server, just the closure-rewrite path):
// an async arrow assigned to a const, invoked twice with an outer-scope
// mutable capture in between.

let count = 0;

const inc = async (): Promise<number> => {
  count += 1;
  await Promise.resolve();
  return count;
};

(async () => {
  const a = await inc();
  const b = await inc();
  const c = await inc();
  console.log(a + "," + b + "," + c);
})();
