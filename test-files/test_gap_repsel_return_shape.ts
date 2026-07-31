// Representation-selection Phase 3b, #7034 §4: return-shape facts
// (RFC docs/representation-selection-rfc.md §5.5-§5.7,
// collectors/ptr_shape_returns.rs).
//
// Behavioural guard for the two halves of the proof. Both must be BYTE-EXACT
// against Node — an optimization that changes an observable answer is a
// miscompile, and every case below is one the pre-#7034 compiler left on the
// guarded protocol, so a divergence here is attributable.
//
// The promotions this file is *about* are asserted structurally, not here:
// `perry test-files/test_gap_repsel_return_shape.ts --opt-report` must list
// `producedRec`, `shaped`, `survivor`, `acc`, `poisoned` and friends as
// `Ptr<Shape>`. A green run of this file with zero promotions would be a
// vacuous pass (#7024/#7025), so the count is checked in review, not inferred.
//
// Covered:
//  1. producer-side: a contained local whose only escape is `return o`,
//  2. caller-side: `const r = producer(...)` as rule-1 provenance,
//  3. object-literal producers (`return { k: v }` -> __AnonShape_*),
//  4. the numeric-field STAND-DOWN: NaN/Infinity/-0 stored by the producer
//     into a field whose constructor store was a plain finite number, read
//     back by the caller — the caller never saw the producer's store,
//  5. GC movement between the call and the field reads (the tagged-at-rest
//     slot must be re-derived and rewritten, RFC §5.6),
//  6. `finally` running after the return value is computed,
//  7. producers that must NOT carry a fact: an aliased cache, a
//     fall-through-to-undefined path, an indirect callee.

class Rec {
  id: number;
  name: string;
  score: number;
  constructor(id: number, name: string, score: number) {
    this.id = id;
    this.name = name;
    this.score = score;
  }
  bump(by: number): number {
    this.score = this.score + by;
    return this.score;
  }
}

// 1 + 2. The accumulator idiom: contained local, single escape is the return.
function producedRec(i: number): Rec {
  const r = new Rec(i, "r" + i, 0);
  r.score = r.id * 1.5;
  r.score = r.score + 0.25;
  return r;
}

// Same, but with a method call on the returned local. `Rec.bump` is denied by
// rule 3 (this-flow) today for reasons unrelated to the return position, so
// this one stays on the guarded protocol — it is here to pin that the two
// paths still agree.
function bumpedRec(i: number): Rec {
  const r = new Rec(i, "b" + i, 0);
  r.bump(i * 0.5);
  return r;
}

function consumeRec(i: number): string {
  const r = producedRec(i);
  r.score = r.score + 1;
  return r.name + ":" + r.score.toFixed(3) + ":" + r.id;
}

// A loop-carried accumulator returned at the end — the `totalsRow` shape.
function foldRecs(n: number): Rec {
  const acc = new Rec(0, "acc", 0);
  for (let i = 1; i <= n; i++) {
    acc.id = acc.id + i;
    acc.score = acc.score + i * 0.5;
  }
  acc.name = "acc" + acc.id;
  return acc;
}

// 3. Object-literal producer: `return { ... }` lowers to __AnonShape_*.
interface Shaped {
  key: string;
  value: number;
}

function shapeOne(i: number): Shaped {
  return { key: "k" + i, value: i * 2 };
}

function readShaped(i: number): string {
  const s = shapeOne(i);
  return s.key + "=" + (s.value + 1);
}

// 4. Values the caller's region never saw stored. The constructor stores a
// plain finite number into `v`; the PRODUCER then stores a non-plain-finite
// one. The caller must not fold the constructor's store into its read — its
// bare load has to survive NaN/Infinity/-0 bit patterns, which is why a
// call-seeded candidate never claims `numeric_fields` (the exhaustive
// reachable-store proof does not reach into the producer). The unit test
// `call_to_a_return_shape_producer_is_provenance` pins the claim itself;
// this pins the observable answer.
class Mixed {
  v: number;
  tag: string;
  constructor() {
    this.v = 1;
    this.tag = "m";
  }
}

function makeMixed(kind: number): Mixed {
  const m = new Mixed();
  m.v = 2;
  if (kind === 1) {
    m.v = NaN;
  } else if (kind === 2) {
    m.v = Infinity;
  } else if (kind === 3) {
    m.v = -0;
  }
  return m;
}

function readMixed(kind: number): string {
  const m = makeMixed(kind);
  // Number context on a slot the caller's region never saw stored.
  return m.tag + "|" + m.v + "|" + (m.v + 1) + "|" + Object.is(m.v, -0);
}

// 5. GC movement between the provenance call and the field reads.
//
// The churn must ESCAPE, or scalar replacement deletes it and the arena never
// grows: measured, a 60 000-iteration loop over a non-escaping literal drives
// ZERO collections, which would make every GC arm inert against this file
// (#6942/#6946 — the failure mode `scripts/gc_repsel_matrix.sh` exists to
// report). The sink is module-level and periodically dropped, so the
// allocations are genuinely live and then genuinely dead. Keep the budget in
// sync with the matrix's liveness column.
let churnSink: unknown[] = [];

function churn(i: number): void {
  churnSink.push({ i: i, s: "c" + (i & 1023), a: [i, i + 1] });
  if (churnSink.length > 4096) {
    churnSink = [];
  }
}

function survivesGc(n: number): string {
  // A CALL-SEEDED (#7034 §4) local: the caller's bound slot is the only
  // rewritable root for this object, since the producer's frame is gone.
  const survivor = producedRec(7);
  let sink = 0;
  for (let i = 0; i < n; i++) {
    churn(i);
    // Read AFTER the allocation safepoint, every iteration. If an evacuating
    // scavenge moved `survivor` and the bound slot was not rewritten — or the
    // raw pointer was CSE'd across the safepoint — this observes a stale
    // address.
    sink = sink + survivor.id;
  }
  return survivor.name + "/" + survivor.score.toFixed(2) + "/" + sink;
}

// 6. `finally` runs after the return value is computed but before the caller
// resumes — the ordering the return exemption's soundness argument rests on.
function returnThenFinally(): string {
  const o = new Rec(1, "fin", 10);
  const seen: string[] = [];
  try {
    o.score = 20;
    return o.name + ":" + o.score + ":" + seen.length;
  } finally {
    seen.push("ran");
    o.score = 999;
  }
}

// 7a. An aliased cache: `return CACHE` is not fresh, so no fact — and the
// caller must observe mutations made through the other alias.
let CACHE: Rec | null = null;
function getCached(): Rec {
  if (CACHE === null) {
    CACHE = new Rec(100, "cached", 0);
  }
  return CACHE;
}

// 7b. A producer that can fall through to `undefined`.
function maybeRec(b: boolean): Rec | undefined {
  if (b) {
    return new Rec(5, "maybe", 5);
  }
  return undefined;
}

// 7c. Indirect callee — the binding is a value, not a statically-known name.
const indirect: (i: number) => Rec = producedRec;

const out: string[] = [];

out.push(consumeRec(3));
out.push(consumeRec(0));
const b = bumpedRec(6);
out.push("bumped:" + b.name + ":" + b.score);
out.push(foldRecs(10).name + "/" + foldRecs(10).score);
out.push(readShaped(4));
out.push(readMixed(0));
out.push(readMixed(1));
out.push(readMixed(2));
out.push(readMixed(3));
out.push(survivesGc(120000));
out.push(returnThenFinally());

const c1 = getCached();
c1.id = 42;
const c2 = getCached();
out.push("cache:" + c2.id + ":" + c2.name);

const m1 = maybeRec(true);
out.push("maybe:" + (m1 === undefined ? "none" : m1.name + m1.score));
const m2 = maybeRec(false);
out.push("maybe:" + (m2 === undefined ? "none" : "some"));

const ind = indirect(9);
out.push("indirect:" + ind.name + ":" + ind.score);

for (const line of out) {
  console.log(line);
}
