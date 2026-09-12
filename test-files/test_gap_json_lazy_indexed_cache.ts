// The indexed inline cache serves reads of a MATERIALIZED lazy JSON array.
// Everything here is about the guards that let it do so safely: the array must
// still be an ordinary unforwarded Array, its length mirror on the lazy header
// must agree, holes must fall back, and a prototype override must disable the
// fast path. Run in auto/tape/direct modes, including scheduled moving GC.
// (An accessor descriptor on one index is test_gap_json_lazy_defineproperty_index.ts:
// main cannot honour it on a lazy array in any parser mode, and that is
// tracked as its own gap.)
const pieces: string[] = [];
for (let i = 0; i < 200; i++) {
    pieces.push('{"id":' + i + ',"name":"heap string for record ' + i + '"}');
}
const text = "[" + pieces.join(",") + "]";
const retained: any[] = [];
const sparseRetained: any[] = [];
let sum = 0;

function fullyMaterialize(rows: any): void {
    // A whole-array scan trips the adaptive flip, so later reads go through the
    // installed ordinary array rather than the sparse per-element cache.
    let seen = 0;
    for (let i = 0; i < rows.length; i++) seen += rows[i].id;
    if (seen !== 19900) throw new Error("scan sum changed: " + seen);
}

for (let round = 0; round < 40; round++) {
    const rows: any = JSON.parse(text);
    fullyMaterialize(rows);

    // Repeated reads off the installed array must keep identity and value.
    const saved: any = rows[7];
    for (let repeat = 0; repeat < 16; repeat++) {
        if (rows[7] !== saved || rows[7].id !== 7) throw new Error("identity changed");
        if (rows[199].id !== 199) throw new Error("tail value changed");
        if (rows[200] !== undefined || rows[4294967295] !== undefined) {
            throw new Error("out-of-bounds read");
        }
        sum += rows[7].id + rows[199].id;
    }

    // Growth: the header's length mirror goes stale, so a fast-path read must
    // not report the old length or miss the new element.
    rows.push({id: 200, name: "grown heap string"});
    if (rows.length !== 201) throw new Error("length after growth: " + rows.length);
    if (rows[200].id !== 200) throw new Error("grown element lost");
    if (rows[7] !== saved) throw new Error("identity lost across growth");

    // Shrink, then regrow into holes. Reads of the hole region must consult the
    // prototype chain rather than loading a stale slot.
    rows.length = 32;
    if (rows.length !== 32) throw new Error("length after shrink: " + rows.length);
    if (rows[32] !== undefined || rows[200] !== undefined) {
        throw new Error("stale read past shrink");
    }
    if (rows[31].id !== 31) throw new Error("surviving element lost");
    rows.length = 64;
    if (rows[48] !== undefined) throw new Error("hole must read undefined");

    // Allocate between passes so scheduled moving GC also covers reads taken
    // after the installed array has moved.
    const churn: any = JSON.parse('{"name":"pass ' + round + '"}');
    if (churn.name !== "pass " + round) throw new Error("churn changed");
    retained.push(saved);
}

// The SPARSE tier: an array whose adaptive walk never trips the
// full-materialization flip stays tape-backed, so repeated reads are served
// from the per-element cache instead of an installed array. Never scan this
// one -- a scan would move it onto the materialized tier above.
for (let round = 0; round < 20; round++) {
    const sparse: any = JSON.parse(text);
    const probes: number[] = [0, 1, 63, 64, 65, 127, 128, 199];
    const first: any[] = [];
    for (let p = 0; p < probes.length; p++) first.push(sparse[probes[p]]);
    for (let repeat = 0; repeat < 12; repeat++) {
        for (let p = 0; p < probes.length; p++) {
            const index = probes[p];
            const value: any = sparse[index];
            // Identity must hold across every repeat: a cache hit returns the
            // same object, never a freshly materialized copy.
            if (value !== first[p]) throw new Error("sparse identity changed at " + index);
            if (value.id !== index) throw new Error("sparse value changed at " + index);
            if (value.name !== "heap string for record " + index) {
                throw new Error("sparse name changed at " + index);
            }
            sum += value.id;
        }
        if (sparse[200] !== undefined || sparse[4294967295] !== undefined) {
            throw new Error("sparse out-of-bounds read");
        }
        if (sparse[-1] !== undefined) throw new Error("negative index read");
    }
    // Zero is a legal cached value and its NaN-boxed bits are all zero, so the
    // bitmap -- not the element word -- has to be what proves a slot cached.
    const zeros: any = JSON.parse("[0,0,0,0,0,0,0,0]");
    for (let repeat = 0; repeat < 8; repeat++) {
        if (zeros[3] !== 0 || zeros[7] !== 0) throw new Error("cached zero lost");
        sum += zeros[3];
    }
    // Mutating through the sparse cache must move the array off it correctly.
    sparse[64] = {id: -64, name: "replacement"};
    if (sparse[64].id !== -64) throw new Error("sparse replacement lost");
    if (sparse[65] !== first[4]) throw new Error("neighbour lost across mutation");
    sparseRetained.push(first[2]);
}

// A prototype index override must disable the fast path process-wide: a hole
// read has to find the inherited value, not undefined.
const holed: any = JSON.parse(text);
fullyMaterialize(holed);
holed.length = 8;
holed.length = 16;
(Array.prototype as any)[12] = "from prototype";
if (holed[12] !== "from prototype") throw new Error("prototype override ignored");
if (holed[3].id !== 3) throw new Error("dense read broken under override");
delete (Array.prototype as any)[12];
if (holed[12] !== undefined) throw new Error("prototype override not retired");

for (let round = 0; round < retained.length; round++) {
    const saved: any = retained[round];
    if (saved.id !== 7 || saved.name !== "heap string for record 7") {
        throw new Error("retained element changed");
    }
    sum += saved.id;
}
// Elements handed out by the sparse tier must survive every later collection
// with their identity and contents intact.
for (let round = 0; round < sparseRetained.length; round++) {
    const saved: any = sparseRetained[round];
    if (saved.id !== 63 || saved.name !== "heap string for record 63") {
        throw new Error("retained sparse element changed");
    }
    sum += saved.id;
}
console.log("lazy-indexed-cache", retained.length, sparseRetained.length, sum);
