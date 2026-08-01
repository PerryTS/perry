// Issue #7142: routing the class-id dispatch tower to a proven-`this` clone is
// sound ONLY behind an inline keys check.
//
// `js_object_delete_field` compacts an object's packed inline slots while
// PRESERVING `class_id` (`object/delete_rest.rs`): on `class Row { a; b; c }`,
// `delete row.b` moves `c` from slot 2 into slot 1 and shortens `field_count`.
// So a dispatch-tower case that matched `class_id` has NOT proved the layout,
// and the clone's bare fixed-slot loads would read the wrong slot. The
// compaction installs a freshly CLONED keys array, which is what the inline
// pointer compare against `@perry_class_keys_*` catches — the receiver falls to
// the generic by-name path and reads the right values.
//
// The `delete` lives in THIS module while the class and its dispatcher live in
// `_helpers/pshape_tower_delete_rows.ts`, on purpose: the `delete` shape
// barrier that stands the analysis down is module-scoped while the receivers
// alias across modules (#7143). A same-module `delete` would disable the clone
// entirely and this test would pass vacuously.
//
// With a class-id-only route this prints `103,NaN,309,412`. Compared
// byte-for-byte against `node --experimental-strip-types`.

import { Row, pickAll } from "./_helpers/pshape_tower_delete_rows.ts";

const rows: Row[] = [];
for (let i = 0; i < 4; i++) {
    rows.push(new Row(i + 1, (i + 1) * 10, (i + 1) * 3));
}

// Baseline: every row still carries the class's canonical keys array.
console.log("before:", pickAll(rows).join(","));

// The compaction `class_id` cannot see.
delete (rows[1] as any).b;

console.log("after:", pickAll(rows).join(","));
console.log("b:", (rows[1] as any).b);
console.log("c:", rows[1].c);
console.log("keys:", Object.keys(rows[1] as any).join("|"));

// A second delete on a different row, to show the check is per-receiver rather
// than a process-wide latch: rows 0, 2 and 3 keep answering from the clone.
delete (rows[3] as any).a;
console.log("after2:", pickAll(rows).join(","));
console.log("keys3:", Object.keys(rows[3] as any).join("|"));
