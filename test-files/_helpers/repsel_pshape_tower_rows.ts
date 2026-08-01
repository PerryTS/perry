// Helper for `test_gap_repsel_pshape_tower_delete.ts` (#7142).
//
// This module deliberately contains NO `delete` (and no other §5.2 shape
// barrier). That is the point: the barrier that stands representation-selection
// down is collected per MODULE (`collect_module_dispatch_facts`), so the
// routing decision made here is taken with no knowledge of the `delete` the
// importing module performs on these very instances (#7143). Only the inline
// keys check emitted at the call site can catch it.
//
// Keep `pick()` reading slot 0 and slot 2 with slot 1 skipped: deleting `b`
// compacts `c` from slot 2 down into slot 1, so a routing decision made on
// `class_id` alone reads a slot that is now `undefined`.

export class Row {
    a: number;
    b: number;
    c: number;

    constructor(a: number, b: number, c: number) {
        this.a = a;
        this.b = b;
        this.c = c;
    }

    // Two declared-field sites, which is what makes routing the dispatch tower
    // to the proven-`this` clone profitable at all.
    pick(): number {
        return this.a * 100 + this.c;
    }
}

// `r` inside the `map` callback has no static class, so `r.pick()` lowers to
// the class-id switch tower (`idispatch.*`) rather than to either of the two
// guard-dominated routing sites. Same shape as
// `benchmarks/app-patterns/kernels/batch.ts`'s `rows.map((r) => r.rescore(1.5))`.
export function pickAll(rows: Row[]): number[] {
    return rows.map((r) => r.pick());
}
