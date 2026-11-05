// Map/Set `for…of` with deletes, clears and re-adds inside the body. Every
// entry that is neither deleted nor already visited must be visited exactly
// once, in insertion order, and an entry re-added after a delete is visited
// again at the end — ECMA-262 iterates the live [[MapData]] list in place.
//
// Before the epoch-based cursor rebase, the raw-index walk recovered from a
// squeeze by reading `cursor-1`, which assumed ONE hole had been removed:
// deleting several already-visited entries plus the current one in a single
// body skipped entries, and enough of them ended the loop early. The same
// reader compaction also cost O(n) per delete while a loop was open.

function runMap(label: string, n: number, atKey: string, del: string[]): void {
  const m = new Map<string, number>();
  for (let i = 0; i < n; i++) m.set("k" + i, i);
  const seen: string[] = [];
  for (const [k] of m) {
    seen.push(k);
    if (k === atKey) for (const d of del) m.delete(d);
  }
  console.log(label, seen.length, seen.join(","));
}

function runSet(label: string, n: number, atKey: string, del: string[]): void {
  const s = new Set<string>();
  for (let i = 0; i < n; i++) s.add("k" + i);
  const seen: string[] = [];
  for (const k of s) {
    seen.push(k);
    if (k === atKey) for (const d of del) s.delete(d);
  }
  console.log(label, seen.length, seen.join(","));
}

const first21: string[] = [];
for (let i = 0; i <= 20; i++) first21.push("k" + i);

runMap("map-1hole", 10, "k3", ["k3"]);
runSet("set-1hole", 10, "k3", ["k3"]);
runMap("map-visited-only", 10, "k3", ["k0", "k1", "k2"]);
runMap("map-3holes", 10, "k3", ["k0", "k1", "k3"]);
runSet("set-3holes", 10, "k3", ["k0", "k1", "k3"]);
runMap("map-ahead", 10, "k3", ["k3", "k4"]);
runMap("map-21holes", 40, "k20", first21);
runSet("set-21holes", 40, "k20", first21);

// delete + re-add of the current key: it moves to the end and is visited
// there, and nothing in between is skipped.
{
  const m = new Map<string, number>([["a", 1], ["b", 2], ["c", 3]]);
  const seen: string[] = [];
  for (const [k, v] of m) {
    seen.push(k + v);
    if (k === "b" && v === 2) { m.delete("b"); m.set("b", 20); }
  }
  console.log("map-readd", seen.join(","));
}

// clear() mid-walk: the list is emptied in place, so the walk continues with
// whatever is appended afterwards.
{
  const m = new Map<string, number>([["a", 1], ["b", 2], ["c", 3]]);
  const seen: string[] = [];
  for (const [k] of m) {
    seen.push(k);
    if (k === "a") { m.clear(); m.set("z", 9); }
  }
  console.log("map-clear", seen.join(","));
  const s = new Set<string>(["a", "b", "c"]);
  const seen2: string[] = [];
  for (const k of s) {
    seen2.push(k);
    if (k === "a") { s.clear(); s.add("z"); }
  }
  console.log("set-clear", seen2.join(","));
}

// Iterator objects (not the for-of fast path) recover the same way.
{
  const m = new Map<string, number>();
  for (let i = 0; i < 40; i++) m.set("k" + i, i);
  const it = m.keys();
  const seen: string[] = [];
  for (let r = it.next(); !r.done; r = it.next()) {
    seen.push(r.value);
    if (r.value === "k20") for (const d of first21) m.delete(d);
  }
  console.log("map-iter-21holes", seen.length, seen[seen.length - 1]);
  const s = new Set<string>();
  for (let i = 0; i < 40; i++) s.add("k" + i);
  const si = s.values();
  const seen2: string[] = [];
  for (let r = si.next(); !r.done; r = si.next()) {
    seen2.push(r.value);
    if (r.value === "k20") for (const d of first21) s.delete(d);
  }
  console.log("set-iter-21holes", seen2.length, seen2[seen2.length - 1]);
}

// Mutation during iteration must stay linear: 50k entries, 12.5k
// delete+re-add pairs inside the walk. Correctness is checked by the sums;
// the timing gate lives in the benchmark suite.
{
  const m = new Map<string, number>();
  for (let i = 0; i < 50_000; i++) m.set("key_" + i, i);
  let rounds = 0;
  for (const [k, v] of m) {
    if ((v & 3) === 0 && v < 50_000) { m.delete(k); m.set(k, v + 1); }
    rounds++;
  }
  let sum = 0;
  for (const v of m.values()) sum = (sum + v) % 1_000_000_007;
  console.log("map-churn", m.size, rounds, sum);
}
