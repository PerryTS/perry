// #7154: publishing a pointer into a GC-traced payload slot must RECORD it.
//
// Three runtime helpers used to write a pointer into a slot the collector
// traces with a raw `*slot = bits`, skipping `layout_note_slot` and the write
// barrier. A heap object is allocated `GC_LAYOUT_POINTER_FREE` and only leaves
// that state when a store RECORDS a pointer; `heap_payload_slot_selection`
// short-circuits on `POINTER_FREE` and skips the WHOLE payload without
// consulting any mask. So the child was never traced: the evacuating young-gen
// minor neither kept it alive nor rewrote the slot when it moved, and the next
// read of the slot returned a pointer into reclaimed nursery memory.
//
// LIVE BY CONSTRUCTION. Each subject is held across escaping allocation churn
// heavy enough to reach the collector, and is READ AFTER the churn — a
// non-moving collection cannot expose this, so the evacuating arms are the ones
// that bite. Keep the churn budgets in sync with the matrix harness: if a
// collector change stops this file collecting, the matrix reports its cells
// UNVER rather than green.

const sink: unknown[] = new Array(1024);

function churn(n: number): void {
    for (let i = 0; i < n; i++) {
        sink[i & 1023] = { a: i, b: "s" + (i & 7), c: [i, i + 1] };
    }
}

// ---------------------------------------------------------------------------
// 1. Object literal that interleaves a `...spread` with a `this`-reading
//    method. That lowering routes through the runtime's
//    `js_object_set_method_by_name` (#809), which patches the closure's
//    reserved `this` capture slot with the receiver. A raw patch left the
//    closure POINTER_FREE, so the receiver was untraced and `this.kind` later
//    read reclaimed memory.
// ---------------------------------------------------------------------------
interface Described {
    kind: string;
    n: number;
    describe(): string;
}

const base = { kind: "base", extra: 7 };
const described: Described[] = [];
for (let r = 0; r < 40; r++) {
    const o = {
        ...base,
        n: r,
        describe(): string {
            return this.kind + ":" + this.n;
        },
    };
    described.push(o);
    churn(3000);
}

let spreadAcc = "";
for (let i = 0; i < described.length; i++) {
    spreadAcc += described[i].describe() + "|";
}
console.log("spread-method:", spreadAcc.length, described[0].describe(), described[39].describe());

// ---------------------------------------------------------------------------
// 2. Computed symbol-keyed method that reads `this`. Same reserved-`this`-slot
//    patch, through `js_object_set_symbol_method`.
// ---------------------------------------------------------------------------
const prim = Symbol.toPrimitive;
const boxes: { value: number }[] = [];
for (let r = 0; r < 40; r++) {
    const b = {
        value: r,
        [prim](): number {
            return this.value * 2 + 1;
        },
    };
    boxes.push(b);
    churn(3000);
}

let symbolTotal = 0;
for (let i = 0; i < boxes.length; i++) {
    symbolTotal += Number(boxes[i]);
}
console.log("symbol-method:", symbolTotal);

// ---------------------------------------------------------------------------
// 3. WeakMap OVERWRITE of an existing key. The insert path was barriered; the
//    overwrite wrote the new value into the entry's field 1 (+40 — the offset
//    #7154's diagnostic scan reports) with a raw store. Once the entry is
//    tenured, that is a missing remembered-set insert: the young replacement
//    value is invisible to the next minor.
// ---------------------------------------------------------------------------
const wm = new WeakMap<object, { v: number; tag: string }>();
const keys: object[] = [];
for (let i = 0; i < 40; i++) {
    const k = { id: i };
    keys.push(k);
    wm.set(k, { v: -1, tag: "seed" });
}
churn(60000); // age the entries out of the nursery
for (let i = 0; i < keys.length; i++) {
    wm.set(keys[i], { v: i, tag: "t" + i });
}
churn(60000);

let wmSum = 0;
let wmTags = 0;
for (let i = 0; i < keys.length; i++) {
    const e = wm.get(keys[i]);
    if (e === undefined) {
        throw new Error("weakmap entry lost at " + i);
    }
    wmSum += e.v;
    wmTags += e.tag.length;
}
console.log("weakmap-overwrite:", wmSum, wmTags);

// Keep the sink observably live so the churn allocations cannot be elided.
console.log("sink:", sink.length, typeof sink[0]);
