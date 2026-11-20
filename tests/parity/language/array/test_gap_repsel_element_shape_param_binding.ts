// repsel #7766: the element-shape versioned loop clone through a FUNCTION
// BOUNDARY, in the element-binding spelling.
//
// A typed `P[]` parameter has no static provenance — the caller can pass
// anything — so the clone's preheader establishes the element shape at run
// time (`js_array_ensure_element_shape`) and the declared type only names the
// class id to check against. These cases are the ways a caller can make the
// declared type a LIE; every one must take the slow path and print exactly
// what node prints. The binding spelling (`const r = ps[i]` / `for…of`) is
// the shape the matcher learned in #7766 — the direct `ps[i].x` spelling has
// its own file (test_gap_repsel_element_shape_loop_clone.ts).

class P {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

class PSub extends P {
  z: number;
  constructor(x: number, y: number, z: number) {
    super(x, y);
    this.z = z;
  }
}

// The binding spelling, through a parameter.
function totalBinding(ps: P[]): number {
  let s = 0;
  for (let i = 0; i < ps.length; i++) {
    const r = ps[i];
    s += r.x + r.y;
  }
  return s;
}

// The for…of spelling — desugars to the binding spelling.
function totalOf(ps: P[]): number {
  let s = 0;
  for (const p of ps) {
    s += p.x + p.y;
  }
  return s;
}

function make(n: number): P[] {
  const a: P[] = [];
  for (let i = 0; i < n; i++) a.push(new P(i, i + 1));
  return a;
}

// ---------------------------------------------------------------------------
// 1. Honest caller — the hot path both spellings exist for.
// ---------------------------------------------------------------------------
const honest = make(64);
console.log("binding:", totalBinding(honest));
console.log("for-of:", totalOf(honest));
// Second visit runs the O(1) confirm path.
console.log("binding-again:", totalBinding(honest));

// ---------------------------------------------------------------------------
// 2. Mixed array — an element of a different class. The guard must decline
//    and the reads go by-name, exactly as node reads them.
// ---------------------------------------------------------------------------
const mixed: P[] = make(8);
mixed.push({ x: 100, y: 200 } as unknown as P);
mixed.push(new P(8, 9));
console.log("mixed binding:", totalBinding(mixed));
console.log("mixed for-of:", totalOf(mixed));

// ---------------------------------------------------------------------------
// 3. Subclass elements — extra fields, different class id. Field VALUES are
//    inherited-compatible, so the sums agree with node either way; the guard
//    must still not treat PSub as P.
// ---------------------------------------------------------------------------
const subs: P[] = [];
for (let i = 0; i < 8; i++) subs.push(new PSub(i, i * 2, i * 3));
console.log("subclass binding:", totalBinding(subs));
console.log("subclass for-of:", totalOf(subs));

// One P mixed among PSubs: no homogeneous class either way.
subs.push(new P(50, 60));
console.log("subclass+base binding:", totalBinding(subs));

// ---------------------------------------------------------------------------
// 4. Plain object literals of the same SHAPE — same keys, not the class.
// ---------------------------------------------------------------------------
const literals = [
  { x: 1, y: 2 },
  { x: 3, y: 4 },
  { x: 5, y: 6 },
] as unknown as P[];
console.log("literal binding:", totalBinding(literals));
console.log("literal for-of:", totalOf(literals));

// ---------------------------------------------------------------------------
// 5. A hole — `undefined` element. Node throws reading `.x` of undefined;
//    Perry must throw the same way, never mask the hole into a pointer.
// ---------------------------------------------------------------------------
const holey: P[] = make(4);
delete (holey as unknown as Record<number, P>)[2];
try {
  console.log("holey binding:", totalBinding(holey));
} catch (e) {
  console.log("holey binding threw:", e instanceof TypeError);
}
try {
  console.log("holey for-of:", totalOf(holey));
} catch (e) {
  console.log("holey for-of threw:", e instanceof TypeError);
}

// ---------------------------------------------------------------------------
// 6. Mutation between calls — the array a previous call verified is grown
//    with a foreign element afterwards; the next call must re-verify.
// ---------------------------------------------------------------------------
const mutated = make(16);
console.log("pre-mutate:", totalBinding(mutated));
mutated.push({ x: 1000, y: 2000 } as unknown as P);
console.log("post-mutate:", totalBinding(mutated));

// ---------------------------------------------------------------------------
// 7. Mutation mid-loop — a body that writes the array does not match the
//    clone at all; it must still be exactly node-correct.
// ---------------------------------------------------------------------------
function sumAndTruncate(ps: P[]): number {
  let s = 0;
  for (let i = 0; i < ps.length; i++) {
    const r = ps[i];
    s += r.x;
    if (i === 2) ps.length = 4;
  }
  return s;
}
console.log("mid-loop truncate:", sumAndTruncate(make(16)));

// ---------------------------------------------------------------------------
// 8. Non-array caller — the brand test's job.
// ---------------------------------------------------------------------------
const fake = {
  length: 3,
  0: new P(1, 1),
  1: new P(2, 2),
  2: new P(3, 3),
} as unknown as P[];
console.log("fake-array binding:", totalBinding(fake));

// ---------------------------------------------------------------------------
// 9. The binding observed as a value elsewhere (declines the clone; must
//    still be correct): r escapes into a comparison.
// ---------------------------------------------------------------------------
function maxX(ps: P[]): number {
  let best = -1;
  for (let i = 0; i < ps.length; i++) {
    const r = ps[i];
    if (r.x > best) best = r.x;
  }
  return best;
}
console.log("escaping binding:", maxX(honest));

// ---------------------------------------------------------------------------
// 10. Empty array through the boundary — zero iterations, both spellings.
// ---------------------------------------------------------------------------
console.log("empty binding:", totalBinding([]));
console.log("empty for-of:", totalOf([]));
