// #7988: `Array.prototype` / `Object.prototype` are memoized as RAW ADDRESSES,
// and the realm they name is per-thread — `js_get_global_this` bootstraps one
// `globalThis` per thread, so every `perry/thread` agent has its own
// `Array.prototype` and its own `Object.prototype` in its own arena.
//
// The memo cells used to be process-global `AtomicUsize` statics, so they
// missed only once per PROCESS: the first thread to touch either intrinsic
// decided the address for every other agent. This probe pins BOTH directions of
// the resulting realm confusion:
//
//   * LEAK — an agent reading `[1,2,3][4]` walked the MAIN thread's
//     `Array.prototype`, so a main-realm prototype index showed up inside the
//     agent's realm (and dereferenced a `GcHeader` in an arena the agent does
//     not own, which is why the pre-fix run also SIGSEGVs intermittently).
//   * BLINDNESS — the agent's own `Array.prototype[8] = v` was invisible to the
//     agent's own reads, because the read resolved the main thread's prototype.
//
// LIVENESS: the main thread pollutes its OWN realm first, which is what forces
// both addresses to be resolved and memoized before any agent starts. Without
// that the agent is simply the first thread to fill the shared cell and the
// broken tree answers correctly — a vacuous probe. `main:` below prints the
// main-realm values, so a run in which the warm-up did nothing is visible
// rather than silently green.
//
// perry-only (`perry/thread` has no Node equivalent), so this is an
// `test_issue_*` behavioural test, not a byte-for-byte gap test.
import { parallelMap, spawn } from "perry/thread";

const MAIN_ARRAY_INDEX = 4;
const MAIN_OBJECT_INDEX = 5;
const AGENT_ARRAY_INDEX = 8;
const AGENT_OBJECT_INDEX = 9;

// 1. The main thread pollutes ITS OWN realm. Both writes go through the
//    `note_*_index_write` hooks, whose `obj == <intrinsic>_prototype_addr()`
//    test resolves and memoizes the address — so from here on the process-wide
//    cells hold MAIN's addresses, which is the state an agent used to inherit.
const mainArrProto = Array.prototype as unknown as Record<number, string>;
const mainObjProto = Object.prototype as Record<number, string>;
mainArrProto[MAIN_ARRAY_INDEX] = "mainArr";
mainObjProto[MAIN_OBJECT_INDEX] = "mainObj";
const mainProbe: number[] = [1, 2, 3];
console.log(
  "main:",
  String(mainProbe[MAIN_ARRAY_INDEX]),
  String(mainProbe[MAIN_OBJECT_INDEX]),
);

// Runs on an agent thread, in the agent's OWN realm:
//   leakArr/leakObj — the main realm's prototype indices must NOT be visible.
//   ownArr/ownObj   — the agent's own prototype indices MUST be visible.
function agentProbe(tag: number): string {
  const before: number[] = [1, 2, 3];
  const leakArr = String(before[MAIN_ARRAY_INDEX]);
  const leakObj = String(before[MAIN_OBJECT_INDEX]);

  const arrProto = Array.prototype as unknown as Record<number, string>;
  const objProto = Object.prototype as Record<number, string>;
  arrProto[AGENT_ARRAY_INDEX] = "arr" + tag;
  objProto[AGENT_OBJECT_INDEX] = "obj" + tag;

  const after: number[] = [1, 2, 3];
  const ownArr = String(after[AGENT_ARRAY_INDEX]);
  const ownObj = String(after[AGENT_OBJECT_INDEX]);
  return leakArr + "/" + leakObj + "/" + ownArr + "/" + ownObj;
}

// 2. A single background OS thread — deterministic, one agent, one realm.
const EXPECTED_SPAWN = "undefined/undefined/arr1/obj1";
const spawned = await spawn((): string => agentProbe(1));
console.log("spawn agent:", spawned, "match:", spawned === EXPECTED_SPAWN);

// 3. Many agents at once, all given the same input, so every worker realm must
//    reach the same answer.
const EXPECTED_MAPPED = "undefined/undefined/arr2/obj2";
const mapped = parallelMap([2, 2, 2, 2], (n: number): string => agentProbe(n));
let allMatch = true;
for (let i = 0; i < mapped.length; i++) {
  if (mapped[i] !== EXPECTED_MAPPED) allMatch = false;
}
console.log("parallelMap count:", mapped.length, "allMatch:", allMatch);

// 4. Isolation runs the other way too: the agents' pollution is in the agents'
//    realms, so the main realm must be exactly as it was.
const mainAfter: number[] = [1, 2, 3];
console.log(
  "main after:",
  String(mainAfter[MAIN_ARRAY_INDEX]),
  String(mainAfter[MAIN_OBJECT_INDEX]),
  String(mainAfter[AGENT_ARRAY_INDEX]),
  String(mainAfter[AGENT_OBJECT_INDEX]),
);
