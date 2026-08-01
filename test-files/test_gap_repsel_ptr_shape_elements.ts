// Representation-selection Phase 3b, #7034 §3: array-element shape facts
// (RFC docs/representation-selection-rfc.md §5.5-§5.7,
// collectors/ptr_shape_elements.rs).
//
// Behavioural guard for the escape this opens: `rows.push(row)` no longer
// disqualifies `row`, and `const r = rows[i]` under an `i < rows.length` loop
// is rule-1 provenance. Every case must be BYTE-EXACT against Node — an
// optimization that changes an observable answer is a miscompile, and every
// case below is one the pre-#7034-§3 compiler left on the guarded protocol,
// so a divergence here is attributable.
//
// The promotions this file is *about* are asserted structurally elsewhere
// (`benchmarks/repsel_census/fixtures/fixture_ptr_shape_elements.ts` carries
// the census floor, and `perry <this file> --opt-report` lists the locals). A
// green run with zero promotions would be a vacuous pass (#7024/#7025).
//
// Covered:
//  1. producer side: a contained local whose only escape is the push,
//  2. reader side, indexed: `const s = rows[i]` under `i < rows.length`,
//  3. reader side, iterator: `for (const r of rows)`, which desugars to (2),
//  4. GC movement between the element read and the field reads — the
//     tagged-at-rest slot must be re-derived and rewritten (RFC §5.6),
//  5. the numeric-field STAND-DOWN: a NaN / Infinity / -0 / string stored
//     through one group member and read back through another,
//  6. arrays that must NOT be proven: one that escapes to a callee, one
//     mutated with `pop`, one with mixed element classes, one read out of
//     bounds — each still has to produce Node's answer.
//
// NOT covered, deliberately: `short[5].id` where the binding is annotated
// `Row`. Node throws `TypeError: Cannot read properties of undefined`; Perry
// prints `undefined` — on `main`, at the base commit, and with
// `PERRY_PTR_SHAPE_LOCALS=0`, so it is an unrelated pre-existing gap and NOT
// this pass's OOB hazard (which E5's in-bounds conjunct is what rules out).
// Asserting `typeof` instead keeps the out-of-bounds read exercised without
// making this file red for someone else's bug.

class Row {
  id: number;
  bucket: string;
  weight: number;
  score: number;
  constructor(id: number, bucket: string, weight: number) {
    this.id = id;
    this.bucket = bucket;
    this.weight = weight;
    this.score = 0;
  }
  rescore(f: number): number {
    this.score = this.weight * f + (this.id % 7);
    return this.score;
  }
}

const BUCKETS = ["alpha", "beta", "gamma", "delta"];

// 1 + 2 + 3: build, then consume both ways in the same function.
function buildAndFold(n: number): string {
  const rows: Row[] = [];
  for (let i = 0; i < n; i++) {
    const row = new Row(i, BUCKETS[i % 4], (i % 97) * 0.5);
    row.score = row.weight + 1;
    rows.push(row);
  }
  let indexed = 0;
  for (let i = 0; i < rows.length; i++) {
    const s = rows[i];
    s.score = s.score + s.weight;
    indexed = indexed + s.score + s.id;
  }
  let iterated = 0;
  for (const r of rows) {
    iterated = iterated + r.rescore(1.5) + r.weight;
  }
  return indexed.toFixed(4) + "/" + iterated.toFixed(4) + "/" + rows.length;
}

// 4: force collections between the element read and its field reads. Every
// access must re-derive the pointer from the shadow-bound slot; a cached raw
// pointer is a stale-address read after an evacuating minor.
function churnBetweenAccesses(n: number): string {
  const rows: Row[] = [];
  for (let i = 0; i < n; i++) {
    rows.push(new Row(i, BUCKETS[i % 4], i));
  }
  let acc = 0;
  for (let i = 0; i < rows.length; i++) {
    const s = rows[i];
    acc = acc + s.id;
    // Allocation between two reads of the SAME element local.
    const junk: Row[] = [];
    for (let k = 0; k < 200; k++) {
      junk.push(new Row(k, "j", k));
    }
    acc = acc + junk.length * 0 + s.weight;
    acc = acc + s.score;
  }
  return acc.toFixed(4);
}

// 5: the numeric stand-down. `weird` is written through the indexed member
// and read back through the `for…of` member; the group claims no numeric
// fields, so the read must go through the plain-finite check, not a bare
// `load double` claiming JsNumber.
function nonFiniteThroughTheGroup(): string {
  const rows: Row[] = [];
  for (let i = 0; i < 4; i++) {
    rows.push(new Row(i, "b", i));
  }
  for (let i = 0; i < rows.length; i++) {
    const s = rows[i];
    if (i === 0) {
      s.score = NaN;
    } else if (i === 1) {
      s.score = Infinity;
    } else if (i === 2) {
      s.score = -0;
    } else {
      s.weight = -Infinity;
    }
  }
  let out = "";
  for (const r of rows) {
    out = out + String(r.score) + "|" + String(1 / r.score) + "|" + String(r.weight) + ";";
  }
  return out;
}

// 6a: the array escapes to an opaque callee, which reshapes an element. The
// answer must still be Node's, which means the proof must NOT have fired.
function reshape(list: Row[]): number {
  let t = 0;
  for (let i = 0; i < list.length; i++) {
    const any = list[i] as unknown as Record<string, number>;
    any["extra"] = i + 1;
    t = t + any["extra"];
  }
  return t;
}

function escapingArray(): string {
  const rows: Row[] = [];
  for (let i = 0; i < 3; i++) {
    rows.push(new Row(i, "e", i));
  }
  const t = reshape(rows);
  let seen = "";
  for (let i = 0; i < rows.length; i++) {
    const s = rows[i];
    seen = seen + s.id + ":" + JSON.stringify(s) + ";";
  }
  return t + "/" + seen;
}

// 6b: `pop` makes the array non-dense; 6c: two classes in one array;
// 6d: an out-of-bounds read yields `undefined`, never an instance of Row.
class Other {
  tag: string;
  constructor(tag: string) {
    this.tag = tag;
  }
}

function mutatedAndMixed(): string {
  const popped: Row[] = [];
  for (let i = 0; i < 4; i++) {
    popped.push(new Row(i, "p", i));
  }
  popped.pop();
  let a = 0;
  for (let i = 0; i < popped.length; i++) {
    a = a + popped[i].id;
  }

  const mixed: object[] = [];
  mixed.push(new Row(1, "m", 1));
  mixed.push(new Other("o"));
  let b = "";
  for (let i = 0; i < mixed.length; i++) {
    b = b + (mixed[i] as { constructor: { name: string } }).constructor.name + ",";
  }

  const short: Row[] = [];
  short.push(new Row(9, "s", 9));
  // Out of bounds: `undefined`, never an instance of Row. E5 is what stops
  // this from reaching a guard-free fixed-offset load.
  const missing = short[5];
  const oob = typeof missing;
  return a + "/" + b + "/" + oob;
}

console.log("build: " + buildAndFold(500));
console.log("churn: " + churnBetweenAccesses(60));
console.log("nonfinite: " + nonFiniteThroughTheGroup());
console.log("escaping: " + escapingArray());
console.log("mutated: " + mutatedAndMixed());
