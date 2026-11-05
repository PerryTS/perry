// #7544 — closed-shape numeric object literals built in a `for` loop now mint
// `Number`-typed anon-shape fields, so they are declared to the collector as a
// raw-f64, POINTER-FREE layout instead of N pointer slots.
//
// Two things are witnessed here, and they pull in opposite directions:
//
//  1. SURVIVAL. A pointer-free declaration tells the collector these slots can
//     never hold a reference. If that were declared for an object that DOES
//     hold one, the child would be stranded — a use-after-free, not a
//     slowdown. So the retained objects here carry a heap string child in a
//     third field, and every one of them is read back after a forced
//     collection.
//
//  2. SELF-HEALING. Perry validates no declared type at runtime (CLAUDE.md,
//     "No runtime type *validation*"), so a field the compiler typed `number`
//     can still receive a pointer through a later dynamic write. The raw-f64
//     store guard must reject those bits, fall back to the boxed setter, and
//     downgrade the descriptor through `layout_note_slot` so the collector
//     starts scanning the slot again. The second half of this test writes heap
//     strings into the numeric fields of already-constructed objects, collects
//     hard, and reads them back.
//
// `gc()` is Perry's global; node only exposes it under `--expose-gc`, so the
// call is guarded and the test is a byte-for-byte parity test either way.
declare const gc: undefined | (() => void);

function collect(): void {
  if (typeof gc === "function") gc();
}

// ── 1. survival ──────────────────────────────────────────────────────────────
// `{ v: i, w: i + 1 }` is the shape that used to mint two Any fields. `tag` is
// a freshly allocated heap string, so the object is genuinely mixed and a
// wrongly-declared pointer-free layout would lose it.
const kept: { v: number; w: number; tag: string }[] = [];
let checksum = 0;
for (let i = 0; i < 40000; i++) {
  const o = { v: i, w: i + 1, tag: "t" + (i % 7) };
  if (i % 4000 === 0) kept.push(o);
  checksum += o.v + o.w;
  if (i % 10000 === 0) collect();
}
collect();
collect();

console.log("checksum", checksum);
console.log("kept", kept.length);
let survived = 0;
for (let k = 0; k < kept.length; k++) {
  const o = kept[k];
  if (o.v === k * 4000 && o.w === k * 4000 + 1 && o.tag === "t" + ((k * 4000) % 7)) {
    survived++;
  }
}
console.log("survived", survived);

// A purely numeric literal — no pointer field at all, so this is the shape
// that actually gets the raw-f64 pointer-free descriptor.
const pure: { a: number; b: number }[] = [];
for (let i = 0; i < 20000; i++) {
  const o = { a: i, b: i * 2 };
  if (i % 2000 === 0) pure.push(o);
}
collect();
let pureOk = 0;
for (let k = 0; k < pure.length; k++) {
  if (pure[k].a === k * 2000 && pure[k].b === k * 2000 * 2) pureOk++;
}
console.log("pureOk", pureOk, "of", pure.length);

// ── 2. self-healing ──────────────────────────────────────────────────────────
// Every one of these objects was constructed with a numeric-typed `n`. Writing
// a freshly allocated heap string over it contradicts the declared layout; the
// store guard must reject the raw-f64 fast path and the descriptor must be
// downgraded so the string is traced as a child from here on.
const healed: { n: number | string }[] = [];
for (let i = 0; i < 20000; i++) {
  const o: { n: number | string } = { n: i };
  if (i % 2000 === 0) healed.push(o);
}
for (let k = 0; k < healed.length; k++) {
  // The concatenation allocates, so this is a real heap string, not an interned
  // literal that would survive by living outside the nursery.
  healed[k].n = "healed-" + k + "-" + k * 3;
}
collect();
collect();

let healedOk = 0;
for (let k = 0; k < healed.length; k++) {
  if (healed[k].n === "healed-" + k + "-" + k * 3 && typeof healed[k].n === "string") {
    healedOk++;
  }
}
console.log("healedOk", healedOk, "of", healed.length);

// Mixed contradiction: half the objects keep their numbers, half take strings.
// Both kinds must read back correctly out of the same shape.
const mixed: { m: number | string }[] = [];
for (let i = 0; i < 20000; i++) {
  const o: { m: number | string } = { m: i };
  if (i % 2000 === 0) mixed.push(o);
}
for (let k = 0; k < mixed.length; k++) {
  if (k % 2 === 1) mixed[k].m = "s" + k + "-" + k;
}
collect();
let mixedOk = 0;
for (let k = 0; k < mixed.length; k++) {
  const want = k % 2 === 1 ? "s" + k + "-" + k : k * 2000;
  if (mixed[k].m === want) mixedOk++;
}
console.log("mixedOk", mixedOk, "of", mixed.length);
