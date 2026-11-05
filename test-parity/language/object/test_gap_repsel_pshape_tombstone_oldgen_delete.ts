// #9200: a tombstone delete on a PROMOTED receiver must keep the successor
// descriptor's keys array rooted across an evacuating minor.
//
// `test_gap_repsel_pshape_tower_delete` caught this through the cross-module
// dispatch tower; this is the minimized non-tower witness, built from the
// confirmed mechanism. The essential order is:
//
//   1. churn past the matrix's `--pressure 8` heap limit with `rows` live
//      across it — the receivers are PROMOTED, so no later minor enumerates
//      them;
//   2. tombstone-delete a key — `publish_object_shape_holes` mints a fresh
//      descriptor for the receiver's nursery-young owned keys clone and
//      retires the armed predecessor in its keys-address sweep, so the arming
//      must come from the stamp funnel or from nowhere;
//   3. churn again — unfixed, the first evacuating minor swept the keys array
//      (a non-carrier record is walked metadata-only and the old receiver is
//      invisible to the minor), `prune_dead_shape_keys` dropped the
//      descriptor, and the receiver came back shapeless.
//
// NO reads between (2) and (3), deliberately: an earlier probe that read the
// rows at every stage did not reproduce, so a read here could mask exactly
// what this witness pins. Unfixed, under `PERRY_OBJECT_TOMBSTONES=1` with an
// evacuating arm, the final line printed `undefined/undefined/undefined[]`
// for both deleted receivers — the silent shapeless wrong answer.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

class Row9200 {
    a: number;
    b: number;
    c: number;

    constructor(a: number, b: number, c: number) {
        this.a = a;
        this.b = b;
        this.c = c;
    }
}

function churn(n: number): number {
    // Escaping allocations: each object is pushed, so nothing is scalar
    // replaced and the nursery genuinely fills.
    const sink: { x: number }[] = [];
    for (let i = 0; i < n; i++) {
        sink.push({ x: i });
    }
    return sink.length;
}

const rows: Row9200[] = [];
for (let i = 0; i < 4; i++) {
    rows.push(new Row9200(i + 1, (i + 1) * 10, (i + 1) * 3));
}

console.log("churn:", churn(200_000));
delete (rows[1] as any).b;
delete (rows[3] as any).a;
console.log("churn2:", churn(200_000));

const parts: string[] = [];
for (let i = 0; i < rows.length; i++) {
    const r: any = rows[i];
    parts.push(r.a + "/" + r.b + "/" + r.c + "[" + Object.keys(r).join("") + "]");
}
console.log("rows:", parts.join(" "));
