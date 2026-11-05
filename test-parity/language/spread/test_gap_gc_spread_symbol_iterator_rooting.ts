// #7498: `[...obj.arr]` when the enclosing function is small enough to INLINE.
//
// TWO LOWERINGS, TWO BUGS. Out of line, the spread calls `js_iterator_to_array`
// directly — that is the drain #7495 rooted, and
// `test_gap_gc_iterator_drain_rooting.ts` is deliberately sized to stay on it.
// Inlined, the spread routes through `array_from_spread_value` instead, which
// resolves `[Symbol.iterator]` through the whole prototype-walk tower first.
// `clone` here is the shrunk twin of that file's `deepClone`: same shape, small
// enough to inline, so it takes the OTHER route.
//
// WHAT WENT STALE. Every frame on that walk held a GC-managed value in a bare
// Rust local across an allocation, and a bare local is exactly what a copying
// minor cannot rewrite:
//
//   * `req_handle_symbol_fallback` (`symbol/get.rs`) read the receiver into a
//     `usize`, then interned a `"_req"` key — an allocation — and read a field
//     off the PRE-move address. `PERRY_GC_PROTECT_FROMSPACE=1` faults there on
//     a 40-byte `GC_TYPE_ARRAY`: the pre-move copy's `keys_array`. It runs on
//     EVERY heap-object symbol read whose own-symbol lookup missed, so the
//     window is unconditional — the fault is 5/5, not intermittent.
//   * `array_prototype_property_value` (`field_get_set/accessors.rs`) took its
//     property name as a `&str` BORROWED OUT OF THE KEY'S `StringHeader`, then
//     allocated three times before reading it. That is the 56-byte
//     `GC_TYPE_STRING` fault in #7498's second trace, and no root can fix it: a
//     `&str` is not a slot the collector can rewrite. The name is copied off
//     the heap before the first allocation instead.
//   * `array_from_spread_value` itself carried the spread RECEIVER through a
//     dozen classification probes and the entire symbol walk, then used it to
//     rebind `this` for the `[Symbol.iterator]()` factory.
//
// NOT OBSERVABLE FROM OUTPUT, BY CONSTRUCTION. Evacuation copies rather than
// zeroes, so a stale address still reads the correct old bytes and this file
// prints the right checksum before and after, on both link modes. Only
// unmapping retired from-space turns the latent read into a signal:
//
//     PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=200 ./out
//
// THE PROTECTED RUN IS NOT CLEAN AFTER #7498. The two faults above are gone;
// the walk now reaches further and faults at two OTHER sites, each filed
// separately — see the corpus note in `test-parity/gc_repsel_corpus.txt`.
// Saying "clean" here would be the overclaim this repo keeps paying for.

const N = 50_000;

interface Item {
    id: number;
    meta: { tags: string[] };
}

const proto: Item = {
    id: 0,
    meta: { tags: ["a", "b", "c"] },
};

// Deliberately small. Growing this function pushes the spread back out of line
// and onto `js_iterator_to_array`, i.e. onto #7495's path instead of this one.
function clone(o: Item): Item {
    return { id: o.id, meta: { tags: [...o.meta.tags] } };
}

let totalIds = 0;
let badTagLen = 0;
let badTagVal = 0;
for (let i = 0; i < N; i++) {
    proto.id = i;
    const c = clone(proto);
    totalIds += c.id;
    if (c.meta.tags.length !== 3) {
        badTagLen++;
    } else if (c.meta.tags[2] !== "c") {
        badTagVal++;
    }
}
console.log("checksum:", totalIds, "badLen", badTagLen, "badVal", badTagVal);
