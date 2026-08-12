// #7988: `Array.prototype` / `Object.prototype` are memoized as RAW ADDRESSES,
// and the realm they name is per-thread — `js_get_global_this` bootstraps one
// `globalThis` per thread, so every `perry/thread` agent has its own
// `Array.prototype` and its own `Object.prototype` in its own arena.
//
// The memo cells used to be process-global `AtomicUsize` statics, so they
// missed only once per PROCESS: the first thread to touch either intrinsic
// decided the address for every other agent. The observable half of that is
// this test — an agent's own `Object.prototype[7] = v` never flipped
// `OBJECT_PROTO_HAS_INDEX`, because the write hook's "is this the prototype?"
// test compared the agent's object against the MAIN thread's address. So the
// hole/OOB read fallback stayed switched off and `[1,2,3][7]` read `undefined`
// on every thread but the first. (The two unobservable halves — reading another
// thread's `GcHeader` on every indexed array write, and one agent's collector
// rewriting a cell that names another agent's heap — are covered by the
// perry-runtime unit test `a_second_agents_prototype_addresses_are_its_own`.)
//
// LIVENESS: the main thread warms both intrinsics BEFORE any agent starts. That
// is what makes the probe discriminating rather than lucky — without it the
// worker might be the thread that fills the shared cell, and the test would
// pass on the broken tree.
//
// perry-only (`perry/thread` has no Node equivalent), so this is an
// `test_issue_*` behavioural test, not a byte-for-byte gap test.
import { parallelMap, spawn } from "perry/thread";

// 1. Warm THIS realm's memoized intrinsic addresses. `main[1] = 9` runs
//    `note_array_index_write`, which resolves `Array.prototype`'s address;
//    `main[7]` runs the OOB fallback, which resolves `Object.prototype`'s.
const main: number[] = [1, 2, 3];
main[1] = 9;
console.log("main warm:", main[1], String(main[7]));

// Runs on an agent thread. Pollutes THIS agent's own realm intrinsics, then
// reads through an ordinary array's prototype chain:
//   a[8] -> Array.prototype[8]
//   a[7] -> Array.prototype (miss) -> Object.prototype[7]
function agentProbe(tag: number): string {
  const objProto = Object.prototype as Record<number, string>;
  const arrProto = Array.prototype as unknown as Record<number, string>;
  objProto[7] = "obj" + tag;
  arrProto[8] = "arr" + tag;
  const a: number[] = [1, 2, 3];
  return String(a[7]) + "/" + String(a[8]);
}

// 2. A single background OS thread — deterministic, one agent, one realm.
const spawned = await spawn((): string => agentProbe(1));
console.log("spawn agent:", spawned, "expected: obj1/arr1");

// 3. Many agents at once, all given the same input, so every worker realm must
//    reach the same answer as the one above.
const mapped = parallelMap([2, 2, 2, 2], (n: number): string => agentProbe(n));
let allMatch = true;
for (let i = 0; i < mapped.length; i++) {
  if (mapped[i] !== "obj2/arr2") allMatch = false;
}
console.log("parallelMap count:", mapped.length, "allMatch:", allMatch);

// 4. Isolation runs the other way too: an agent's prototype pollution is in the
//    agent's realm, so the MAIN thread's arrays must be unchanged.
const after: number[] = [1, 2, 3];
console.log("main after:", String(after[7]), String(after[8]));
