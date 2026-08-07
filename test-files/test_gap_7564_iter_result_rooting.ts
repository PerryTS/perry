// #7564 rooting half: four of the five runtime `make_iter_result` copies built
// the `{ value, done }` object with every intermediate in a bare Rust local:
//
//   let obj       = js_object_alloc(0, 2);                 // (1) can collect
//   let value_key = js_string_from_bytes(b"value", 5);     // (2) `obj` now stale
//   let done_key  = js_string_from_bytes(b"done", 4);      // (3) + `value_key`
//   let keys      = js_array_alloc(2);                     // (4) + `done_key`
//   js_array_push(keys, value_key);                        // (5) + `keys`
//   js_array_push(keys, done_key);
//   js_object_set_keys(obj, keys);      // ← `obj`/`keys` may be from-space
//   js_object_set_field(obj, 0, value); // ← `value` (a PARAMETER) too
//
// Five allocations, each a collection point, and NOTHING between them is a GC
// root. A copying minor landing in any of windows (1)–(5) moves the objects the
// earlier locals name and rewrites only slots it can see; a Rust local is not
// one. The function then publishes an object whose keys array — or whose key
// strings, or whose `value` — is a retired from-space address.
//
// The fifth copy (`array/iter_object.rs::build_iter_result`) was rooted by
// #7475 and is the model the other four now follow.
//
// This is deliberately NOT the Map/Set `for-of` shape: #7561 lowers that to a
// direct index walk that never builds a result object at all. It drives the
// paths that still reach the runtime constructor — `.values()`/`.entries()`
// method iterators pulled by hand, string and typed-array iteration, and the
// `Iterator.from(...).map(...)` helper chain — while allocating hard enough
// that a copying minor lands inside the constructor.
//
// Run under `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1`, compiled with
// `PERRY_GC_MOVING_LOOP_POLLS=1`, and confirm `PERRY_GC_DIAG=1` prints a
// `[gc-fromspace-protect] mode=... retired_set=#N` line — a run with zero
// copying minors protects nothing and proves nothing.

const N = 4000;

// Per-iteration garbage so the nursery actually fills and minors actually run.
function churn(i: number): string {
    return "pad-" + i + "-" + (i * 7919) + "-" + (i % 13);
}

let acc = 0;
let bad = 0;

// ── 1. Map/Set method iterators, pulled by hand ─────────────────────────────
for (let i = 0; i < N; i++) {
    const m = new Map<string, number>([
        ["a" + (i % 7), i],
        ["b" + (i % 5), i + 1],
        ["c", i + 2],
    ]);
    const it = m.entries();
    let step = it.next();
    while (!step.done) {
        const pair = step.value as [string, number];
        // Read the key string back off the result — a stale key string or a
        // stale keys array shows up right here.
        if (typeof pair[0] !== "string") bad++;
        acc += pair[1];
        churn(i);
        step = it.next();
    }
    const s = new Set([i, i + 1, i + 2]);
    const sv = s.values();
    let sstep = sv.next();
    while (!sstep.done) {
        acc += sstep.value as number;
        churn(i + 1);
        sstep = sv.next();
    }
}

// ── 2. String iterator, pulled by hand ──────────────────────────────────────
for (let i = 0; i < N; i++) {
    const str = "abcdefg" + (i % 10);
    const si = str[Symbol.iterator]();
    let st = si.next();
    let chars = 0;
    while (!st.done) {
        if (typeof st.value !== "string") bad++;
        chars++;
        churn(i);
        st = si.next();
    }
    if (chars !== 8) bad++;
    acc += chars;
}

// ── 3. Typed-array / Buffer iterators ───────────────────────────────────────
for (let i = 0; i < N; i++) {
    const ta = new Uint8Array([i & 0xff, (i + 1) & 0xff, (i + 2) & 0xff]);
    const te = ta.entries();
    let t = te.next();
    while (!t.done) {
        const pair = t.value as [number, number];
        acc += pair[0] + pair[1];
        churn(i);
        t = te.next();
    }
}

// ── 4. Iterator helpers (`Iterator.from(...).map(...).filter(...)`) ─────────
function* small(n: number): Generator<number, void, undefined> {
    for (let k = 0; k < n; k++) yield k + n;
}
for (let i = 0; i < N; i++) {
    const chain = Iterator.from(small(6))
        .map((x: number) => {
            churn(x + i);
            return x * 2;
        })
        .filter((x: number) => x % 3 !== 0);
    let c = chain.next();
    while (!c.done) {
        acc += c.value as number;
        c = chain.next();
    }
}

// ── 5. Results retained across many later collections ───────────────────────
// A held result whose keys array was left in from-space reads back wrong here.
const kept: Array<IteratorResult<number>> = [];
const keeper = new Set<number>();
for (let i = 0; i < 200; i++) keeper.add(i);
const kv = keeper.values();
for (let i = 0; i < 200; i++) kept.push(kv.next() as IteratorResult<number>);
for (let i = 0; i < N; i++) churn(i);
let keptSum = 0;
for (const k of kept) {
    if (Object.keys(k).join(",") !== "value,done") bad++;
    keptSum += k.value as number;
}

console.log("acc:", acc, "bad:", bad, "keptSum:", keptSum);
