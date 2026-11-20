// #8179: every value `dispatch_uint8_buffer_method` carried across a user
// callback lived in a bare Rust local —
//
//   * the receiver `addr` (re-read as `uint8_get(addr, i)` on the NEXT
//     iteration) and the NaN-boxed `receiver` handed to the callback as its
//     3rd/4th argument,
//   * `map`'s freshly allocated `out` buffer, written after every call,
//   * `sort`/`toSorted`'s `out_addr`, permuted under a user comparator,
//   * `reduce`/`reduceRight`'s `accumulator`, which is a NURSERY object
//     whenever the seed or a callback result is a string/object/array,
//   * and the callback closure itself, which on two of the three entries into
//     this dispatcher had no root at all (gh #6206 / #6081: a closure is
//     non-movable but IS swept, and an arrow at a frameless call site is
//     reachable only through that raw parameter).
//
// The callbacks below allocate inside a LOOP on purpose. A back-edge poll is
// the only safepoint reachable from inside user JS (`js_gc_loop_safepoint`),
// so an allocation-free callback gives the seeded GC schedule nothing to fire
// on and the whole file passes vacuously. Run it as
//
//   PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_SEED=7 \
//   PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1 \
//   PERRY_GC_DIAG=1 ./out
//
// (`PERRY_GC_SCHEDULE_SEED` implies forced evacuation, so survivors actually
// MOVE.) Two things must be true before a green run means anything: a
// `[gc-fromspace-protect] … retired_set=#N` line must appear, and the
// instrument's own exit verdict must report `copying_minors` and
// `moved_objects` above zero — a run with zero copying minors protects
// nothing. Pre-fix this file dies on the FIRST scheduled collection
// (`[gc-schedule] FAILURE (signal 10) … safepoints=1`); post-fix the same seed
// reports `safepoints=5306 copying_minors=5306 moved_objects=25760` and
// exits 0.
//
// BOTH dispatch entries are exercised. A statically typed receiver reaches
// `dispatch_uint8_buffer_method` through `dispatch_buffer_method`'s catch-all,
// while a fused `js_array_*` helper reaches it through
// `array::buffer_receiver_dispatch` — and only the latter had rooted the
// receiver and the callback at the boundary.
//
// Perry's `new Uint8Array([…])` is a Buffer (`BufferHeader`), which is what
// makes this dispatcher run at all; see #8137/#8173.

function churn(n: number): string {
    let s = "";
    for (let k = 0; k < n; k++) {
        s = "g" + (k + n) + ":" + s.length;
    }
    return s;
}

const SRC = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5, 8, 9, 7, 9, 3];
const ROUNDS = 300;

let mapMismatch = 0;
let filterMismatch = 0;
let forEachMismatch = 0;
let reduceMismatch = 0;
let reduceRightMismatch = 0;
let sortMismatch = 0;
let findMismatch = 0;
let protoMapMismatch = 0;
let protoReduceMismatch = 0;

const EXPECT_MAP = "6,2,8,2,10,18,4,12,10,6,10,16,18,14,18,6";
const EXPECT_FILTER = "4,2,6,8";
const EXPECT_REDUCE = "s-3-1-4-1-5-9-2-6-5-3-5-8-9-7-9-3";
const EXPECT_REDUCE_RIGHT = "s-3-9-7-9-8-5-3-5-6-2-9-5-1-4-1-3";
const EXPECT_SORT = "1,1,2,3,3,3,4,5,5,5,6,7,8,9,9,9";

for (let r = 0; r < ROUNDS; r++) {
    const u = new Uint8Array(SRC);

    const doubled = u.map((v: number) => {
        churn(12);
        return (v * 2) % 256;
    });
    if (doubled.join(",") !== EXPECT_MAP) {
        mapMismatch++;
    }

    const evens = u.filter((v: number) => {
        churn(6);
        return v % 2 === 0;
    });
    if (evens.join(",") !== EXPECT_FILTER) {
        filterMismatch++;
    }

    let seen = "";
    u.forEach((v: number, i: number) => {
        churn(4);
        seen = seen + v;
        if (i === 0 && v !== 3) {
            forEachMismatch++;
        }
    });
    if (seen !== "3141592653589793") {
        forEachMismatch++;
    }

    // A STRING accumulator: the one value in this loop that is a nursery
    // object on every single iteration.
    const joined = u.reduce((acc: string, v: number) => {
        churn(8);
        return acc + "-" + v;
    }, "s");
    if (joined !== EXPECT_REDUCE) {
        reduceMismatch++;
    }

    const joinedRight = u.reduceRight((acc: string, v: number) => {
        churn(8);
        return acc + "-" + v;
    }, "s");
    if (joinedRight !== EXPECT_REDUCE_RIGHT) {
        reduceRightMismatch++;
    }

    // The comparator runs over a `Vec<u8>` while `out_addr` sits in a Rust
    // local; `toSorted` makes that local a FRESH buffer rather than the
    // receiver.
    const sorted = u.toSorted((a: number, b: number) => {
        churn(3);
        return a - b;
    });
    if (sorted.join(",") !== EXPECT_SORT) {
        sortMismatch++;
    }

    const found = u.find((v: number) => {
        churn(5);
        return v > 8;
    });
    if (found !== 9) {
        findMismatch++;
    }

    // The reflective entry: `%TypedArray%.prototype` thunk → brand check →
    // the same dispatcher, with NO boundary root on the receiver or callback.
    const protoMapped = Uint8Array.prototype.map.call(u, (v: number) => {
        churn(7);
        return (v + 1) % 256;
    });
    if (Array.prototype.join.call(protoMapped, ",") !== "4,2,5,2,6,10,3,7,6,4,6,9,10,8,10,4") {
        protoMapMismatch++;
    }

    const protoReduced = Uint8Array.prototype.reduce.call(
        u,
        (acc: string, v: number) => {
            churn(6);
            return acc + "." + v;
        },
        "p",
    );
    if (protoReduced !== "p.3.1.4.1.5.9.2.6.5.3.5.8.9.7.9.3") {
        protoReduceMismatch++;
    }
}

console.log("map", mapMismatch);
console.log("filter", filterMismatch);
console.log("forEach", forEachMismatch);
console.log("reduce", reduceMismatch);
console.log("reduceRight", reduceRightMismatch);
console.log("sort", sortMismatch);
console.log("find", findMismatch);
console.log("protoMap", protoMapMismatch);
console.log("protoReduce", protoReduceMismatch);
