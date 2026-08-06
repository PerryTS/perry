// #7475: `{ tags: [...o.meta.tags] }` drains the array iterator through
// `js_iterator_to_array`, and EVERY value that loop carried across a `.next()`
// call used to live in a bare Rust local:
//
//   * the iterator object itself,
//   * the accumulator array it is filling,
//   * the `next` closure, and the `"done"` / `"value"` key strings,
//   * and one level down, the element `value` and the freshly allocated
//     `{ value, done }` object inside `make_iter_result`.
//
// `.next()` allocates that result object on every step, so any of those
// allocations can trigger the copying minor. It moves the values and rewrites
// only the slots it can see; a bare Rust local is not one. A moved iterator
// leaves its pre-move copy in retired from-space, and the next dispatch reads
// THAT copy's field 0 — the backing array — so
// `dispatch_array_iterator_method` called `js_array_length` on a from-space
// address. `PERRY_GC_PROTECT_FROMSPACE=1` faults on it precisely.
//
// STRUCTURE IS LOAD-BEARING, so this file deliberately mirrors
// `benchmarks/app-patterns/kernels/object_deep_clone.ts` where it was found
// rather than reducing further. `deepClone`'s size is what keeps it out of
// line, and an out-of-line spread behind two property hops is the lowering
// that calls `js_iterator_to_array` directly. Shrink the function and the
// spread re-routes through `array_from_spread_value` instead, which has its
// own separate stale-root defect (#7498) and would make this file red for the
// wrong reason.
//
// LIVE BY CONSTRUCTION AND ONLY WHEN SOMETHING MOVES. A non-moving collection
// cannot expose it — the stale address still names the same bytes — which is
// why it surfaced first through the auto-optimize link, whose feature-stripped
// runtime shifts allocation timing enough to make the stale read observable.

const N = 50_000;

interface Inner {
    code: string;
    weight: number;
}

interface Item {
    id: number;
    name: string;
    meta: { created: string; updated: string; tags: string[] };
    inners: Inner[];
}

const proto: Item = {
    id: 0,
    name: "item",
    meta: { created: "2026-01-01", updated: "2026-05-09", tags: ["a", "b", "c"] },
    inners: [
        { code: "x1", weight: 0.5 },
        { code: "x2", weight: 1.5 },
        { code: "x3", weight: 2.5 },
    ],
};

function deepClone(o: Item): Item {
    return {
        id: o.id,
        name: o.name,
        meta: {
            created: o.meta.created,
            updated: o.meta.updated,
            tags: [...o.meta.tags],
        },
        inners: o.inners.map((x) => ({ code: x.code, weight: x.weight })),
    };
}

let totalIds = 0;
let badTagLen = 0;
let badTagVal = 0;
for (let i = 0; i < N; i++) {
    proto.id = i;
    const clone = deepClone(proto);
    totalIds += clone.id;
    if (clone.meta.tags.length !== 3) {
        badTagLen++;
    } else if (clone.meta.tags[2] !== "c") {
        badTagVal++;
    }
}
console.log("checksum:", totalIds, "badLen", badTagLen, "badVal", badTagVal);
