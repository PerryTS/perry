// Liveness fixture for the three canonical-slot census keys (#7035):
// `canonical-i32`, `canonical-u32` and `canonical-str`.
//
// These are the reps `expr/slot_rep.rs` selects. Splitting them into three
// census keys rather than one `canonical-slot` aggregate is deliberate: on
// `batch.ts` the ONLY promotion in the whole program is a canonical `Str`
// local, and an aggregate would have shown "canonical-slot: 1" while hiding
// that I32 and U32 promoted nothing there.
//
// Requirements: plain synchronous function bodies (async / generator bodies
// route locals through the async-to-generator shared cells and are excluded),
// no closure capture of the candidate locals.

function i32Mixer(seed: number): number {
  // Canonical i32: STRAIGHT-LINE locals seeded from an i32 literal and only
  // ever bitwise-updated. Straight-line on purpose — a loop-CARRIED
  // accumulator and a loop counter both route through the parallel-shadow
  // `i32_counter_slots` model instead of selecting the canonical rep, so
  // wrapping this in a `for` would take the fixture to zero without any
  // representation actually regressing.
  let h = 2166136261 | 0;
  const k = 16777619 | 0;
  h = h ^ (seed | 0);
  h = (h << 5) ^ k;
  h = h ^ (h >> 7);
  return h | 0;
}

function u32Mixer(seed: number): number {
  // Canonical u32: `>>> 0` keeps the value observable as unsigned above 2^31.
  let u = (0x9e3779b9 ^ (seed | 0)) >>> 0;
  for (let i = 0; i < 16; i++) {
    u = (u ^ (u << 13)) >>> 0;
    u = (u ^ (u >>> 17)) >>> 0;
  }
  return u >>> 0;
}

function strBuilder(n: number): number {
  // Canonical Str: a string local whose every write is a string.
  let s = "seed";
  for (let i = 0; i < n; i++) {
    s = s + "x";
  }
  return s.length;
}

console.log("canonical:" + (i32Mixer(8) + u32Mixer(8) + strBuilder(8)));
