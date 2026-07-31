// Liveness fixture for canonical selection in a MODULE-INIT body (#7109).
//
// `fixture_canonical_slots.ts` proves the three canonical reps are alive in
// function bodies. This one proves the same thing for the program-entry body,
// and it is deliberately the ONLY program in the corpus that can: it declares
// no function, no method and no closure, so every promotion it reports had to
// come from `codegen/entry.rs`'s FnCtx.
//
// Before #7109 both entry contexts hard-coded
// `repsel_context_allows_canonical_{i32,str}: false`, so this file's counts
// were 0/0/0 no matter what the per-value rules said. That is what makes the
// floors below falsifiable: revert the entry.rs gate and this fixture goes to
// zero on all three keys while every function-body fixture stays green.
//
// Requirements mirrored from the function-body fixture: no closure may capture
// a candidate (there are no closures), and no candidate may be a module global
// (nothing here is exported or read from a function, so nothing is globalized
// into `@perry_global_*`).

// Canonical i32: an index-used loop counter and the index-used bound it is
// compared against. `data[i]` is what makes both index-used; a counter that
// never reaches an array index is rejected by `not_index_used_or_bounded`.
const LIMIT = 64;
const data: number[] = [];
for (let i = 0; i < LIMIT; i++) {
    data[i] = i * 3;
}

let checksum = 0;
for (let i = 0; i < LIMIT; i++) {
    checksum = checksum + data[i];
}

// Canonical u32: every write is a top-level `>>> 0`, so the value stays
// observable as unsigned above 2^31 and the u32 bit pattern round-trips.
let mixed = (0x9e3779b9 ^ LIMIT) >>> 0;
for (let s = 0; s < 8; s++) {
    mixed = (mixed ^ (mixed << 13)) >>> 0;
    mixed = (mixed ^ (mixed >>> 17)) >>> 0;
}

// Canonical Str: a string local whose every write is a string — Phase 3a's
// motivating `+=` self-append shape, which is what `benchmarks/suite/
// 08_string_concat.ts` is and why it promoted nothing before #7109.
let text = "seed";
for (let t = 0; t < LIMIT; t++) {
    text = text + "x";
}

console.log("module_init_canonical:" + checksum + ":" + mixed + ":" + text.length);
