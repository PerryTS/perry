// #7114 — a string-literal operand held across an allocating sibling.
//
// `console.log("acc:" + run(n))` loads the literal's `__perry_init_strings_*`
// handle BEFORE the call and masks it to a pointer AFTER it. The handle global
// is a registered GC root, so the string is never swept and the global is
// rewritten on evacuation — but the register taken beforehand still holds the
// pre-move address. Under enough allocation to drive an evacuating minor the
// concatenation silently dropped or corrupted its prefix and the program exited
// 0. On main this printed an EMPTY LINE where `acc:74999992500000` belongs.
//
// ***WHY THE FIRST LINE IS THE ONE THAT MATTERS.***
// Every string literal in the module is materialized once, together, by
// `__perry_init_strings_*` before any user code runs — so they are all young at
// the same moment and the first evacuating cycle relocates all of them at once.
// After that they live in the old generation and a nursery scavenge cannot move
// them again. So a program gets exactly ONE chance to observe this bug, and it
// is the first literal-plus-allocating-call expression it evaluates. That is
// why `stale` runs first, before anything else has had a chance to collect, and
// why the checks below it are shape coverage rather than a second live probe.
// The exhaustive per-shape coverage is the codegen contract test
// (crates/perry-codegen/tests/temp_root_operand_temporaries.rs); this file is
// the end-to-end proof that the shape really corrupts under a real collection.
//
// Registered in test-parity/gc_repsel_corpus.txt so the GC matrix runs it under
// the evacuating arms and reports whether anything actually moved. A run in
// which nothing moved proves nothing here — a non-moving collection cannot
// produce a stale pointer.

class Rec {
    id: number;
    score: number;
    constructor(id: number, score: number) {
        this.id = id;
        this.score = score;
    }
}

// Escaping churn: the records go into a module-level sink and are dropped in
// batches, so the arena genuinely grows and the survivors are genuinely
// relocatable. Without the sink the allocations are dead on arrival and the
// collector has nothing to move.
let sink: Rec[] = [];
let dropped = 0;

function make(i: number): Rec {
    const r = new Rec(i, 0);
    r.score = r.id * 1.5;
    sink.push(r);
    if (sink.length > 8192) {
        dropped = dropped + sink.length;
        sink = [];
    }
    return r;
}

function run(n: number): number {
    let acc = 0;
    for (let i = 0; i < n; i++) {
        const r = make(i);
        acc = acc + r.score;
    }
    return acc;
}

// Measured on main (M1, `--release`, DEFAULT GC settings, no env at all):
// N = 50 000 drives 0 collections and passes vacuously; N = 100 000 drives 7
// cycles / 590 472 scavenged and still passes; N = 150 000 is the first size at
// which the first `run` reaches an evacuating minor while `"acc:"` is still in
// the nursery, and it prints an empty line. N below is ~3x that threshold so a
// collector retune degrades this to UNVERIFIED (the harness's honest state)
// rather than to a silent pass.
const N = 400000;

// THE PROBE. First statement, first literal use: `"acc:"` is loaded before
// `run` and consumed after it.
console.log("acc:" + run(N));

// The same value with the call hoisted into its own statement — the literal is
// loaded after the collection, which is why this line was always correct and
// the line above was not. Keeping both makes a regression report the
// difference rather than just "the number is wrong".
const hoisted = run(N);
console.log("hoisted:" + hoisted);

// Shape coverage. All four route through `lower_exprs_rooted`, the helper that
// suppressed the literal without re-deriving it; each pairs a literal operand
// with an allocating sibling in a different lowering path.
//   literal on the RIGHT      -> js_value_concat_string
console.log(run(1000) + ":right");
//   literal + literal, allocating sibling further along the chain
console.log("chain:" + run(1000) + ":end");
//   template literal (the parts list is mostly literals)
console.log(`tpl:${run(1000)}:done`);
//   literal argument next to an allocating argument
console.log("args", run(1000), "tail");

console.log("dropped", dropped > 0);
console.log("sink", sink.length > 0);
