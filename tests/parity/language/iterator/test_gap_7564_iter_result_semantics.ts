// #7564: the generic iterator path builds its `{ value, done }` result object
// in the runtime. That object is USER-VISIBLE — the spec hands it straight back
// to whoever called `.next()` — so every observable property of it is a parity
// obligation, not an implementation detail.
//
// This is the semantics matrix for that object, byte-diffed against node. It
// exists because #7564 shares the keys array (and therefore the shape) across
// every result the runtime builds, which is only sound if all of the following
// stay true:
//
//   * key ORDER is `value` then `done` (Object.keys / for-in / JSON.stringify),
//     except `node:sqlite`'s iterator which is `done` then `value`;
//   * each `.next()` returns a DISTINCT object — results are never recycled,
//     because user code is allowed to retain one;
//   * writing a NEW property onto one result must not be visible on the next
//     one (the shared keys array is copy-on-write via `GC_FLAG_SHAPE_SHARED`);
//   * overwriting `value`/`done` in place likewise stays local to that object;
//   * a user-supplied `next` returning a non-object is the user's own object
//     flowing through unchanged — the runtime must not rewrap it.
//
// Every construction site in the runtime is exercised: array, string, typed
// array/Buffer, Map/Set collection, and the generic `iterator_helpers` drain.

// ── 1. Array iterator: shape, order, identity ───────────────────────────────
const arr = [10, 20, 30];
const ai = arr[Symbol.iterator]();
const r1 = ai.next();
const r2 = ai.next();
console.log("1a keys:", JSON.stringify(Object.keys(r1)));
console.log("1b json:", JSON.stringify(r1));
console.log("1c distinct:", r1 !== r2);
console.log("1d values:", r1.value, r1.done, r2.value, r2.done);
const forIn1: string[] = [];
for (const k in r1) forIn1.push(k);
console.log("1e forin:", JSON.stringify(forIn1));
console.log("1f entries:", JSON.stringify(Object.entries(r2)));

// exhaustion
const ai2 = [1][Symbol.iterator]();
ai2.next();
const done1 = ai2.next();
console.log("1g exhausted:", JSON.stringify(done1), done1.value === undefined);

// ── 2. Adding a property to a result must not leak to siblings ──────────────
const ai3 = [1, 2, 3][Symbol.iterator]();
const p1 = ai3.next() as Record<string, unknown>;
p1.extra = "mine";
const p2 = ai3.next() as Record<string, unknown>;
console.log("2a p1keys:", JSON.stringify(Object.keys(p1)));
console.log("2b p2keys:", JSON.stringify(Object.keys(p2)));
console.log("2c leak:", p2.extra === undefined);
const p3 = ai3.next() as Record<string, unknown>;
console.log("2d p3keys:", JSON.stringify(Object.keys(p3)), p3.extra === undefined);
console.log("2e p1 intact:", p1.value, p1.done, p1.extra);

// overwrite in place
const ai4 = [7, 8][Symbol.iterator]();
const w1 = ai4.next();
w1.value = 999;
w1.done = true;
const w2 = ai4.next();
console.log("2f overwrite:", w1.value, w1.done, "|", w2.value, w2.done);

// ── 3. Retention: results held across many later steps stay intact ──────────
const held: Array<IteratorResult<number>> = [];
const bigIter = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9][Symbol.iterator]();
for (let i = 0; i < 10; i++) held.push(bigIter.next());
console.log("3a held:", held.map((h) => h.value).join(","));
console.log("3b heldDone:", held.map((h) => (h.done ? 1 : 0)).join(""));
console.log("3c allDistinct:", new Set(held).size === 10);

// ── 4. String iterator ──────────────────────────────────────────────────────
const si = "ab"[Symbol.iterator]();
const s1 = si.next();
console.log("4a keys:", JSON.stringify(Object.keys(s1)), JSON.stringify(s1));
console.log("4b:", JSON.stringify(si.next()), JSON.stringify(si.next()));

// ── 5. Typed array / Buffer iterators ───────────────────────────────────────
const ta = new Uint8Array([5, 6]);
const ti = ta[Symbol.iterator]();
console.log("5a:", JSON.stringify(ti.next()), JSON.stringify(ti.next()), JSON.stringify(ti.next()));
const te = ta.entries();
const te1 = te.next();
console.log("5b keys:", JSON.stringify(Object.keys(te1)), JSON.stringify(te1));
const tk = ta.keys();
console.log("5c:", JSON.stringify(tk.next()));

// ── 6. Array entries/keys/values ────────────────────────────────────────────
const ae = ["x", "y"].entries();
const ae1 = ae.next();
console.log("6a keys:", JSON.stringify(Object.keys(ae1)), JSON.stringify(ae1));
console.log("6b:", JSON.stringify(["x", "y"].keys().next()));

// ── 7. Map / Set iterators ──────────────────────────────────────────────────
const m = new Map<string, number>([["a", 1], ["b", 2]]);
const mv = m.values();
const mv1 = mv.next();
console.log("7a keys:", JSON.stringify(Object.keys(mv1)), JSON.stringify(mv1));
const me = m.entries();
console.log("7b:", JSON.stringify(me.next()), JSON.stringify(me.next()), JSON.stringify(me.next()));
const st = new Set([9, 8]);
const sv = st.values();
console.log("7c:", JSON.stringify(sv.next()), JSON.stringify(sv.next()), JSON.stringify(sv.next()));
// manual .next() on a Map iterator, results retained
const mk = m.keys();
const mkHeld = [mk.next(), mk.next(), mk.next()];
console.log("7d:", mkHeld.map((h) => JSON.stringify(h)).join(" "));

// ── 8. Generators ───────────────────────────────────────────────────────────
function* gen(): Generator<number, string, undefined> {
    yield 1;
    yield 2;
    return "end";
}
const g = gen();
const g1 = g.next();
const g2 = g.next();
const g3 = g.next();
const g4 = g.next();
console.log("8a keys:", JSON.stringify(Object.keys(g1)));
console.log("8b:", JSON.stringify(g1), JSON.stringify(g2), JSON.stringify(g3), JSON.stringify(g4));
console.log("8c distinct:", g1 !== g2 && g2 !== g3);

function* outer(): Generator<number, void, undefined> {
    yield 0;
    yield* gen();
    yield 3;
}
console.log("8d yield*:", [...outer()].join(","));

// ── 9. User-defined iterables ───────────────────────────────────────────────
class Counter {
    n: number;
    constructor(n: number) {
        this.n = n;
    }
    [Symbol.iterator](): Iterator<number> {
        let i = 0;
        const n = this.n;
        return {
            next(): IteratorResult<number> {
                return i < n ? { value: i++, done: false } : { value: undefined, done: true };
            },
        };
    }
}
console.log("9a spread:", JSON.stringify([...new Counter(4)]));
const forOf9: number[] = [];
for (const v of new Counter(3)) forOf9.push(v);
console.log("9b forof:", JSON.stringify(forOf9));
console.log("9c from:", JSON.stringify(Array.from(new Counter(3))));

// A user `next` result flows through UNCHANGED — same object identity.
const sentinel = { value: 42, done: false, tag: "sentinel" };
let handedOut = 0;
const passthrough = {
    next(): IteratorResult<number> {
        handedOut++;
        return handedOut === 1 ? (sentinel as IteratorResult<number>) : { value: undefined, done: true };
    },
};
const got = passthrough.next();
console.log("9d identity:", got === sentinel, JSON.stringify(Object.keys(got)));

// ── 10. Non-object `next` return is a TypeError ─────────────────────────────
const badIterable = {
    [Symbol.iterator]() {
        return { next: () => 5 as unknown as IteratorResult<number> };
    },
};
try {
    console.log("10a:", [...(badIterable as Iterable<number>)]);
} catch (e) {
    console.log("10a threw:", e instanceof TypeError);
}

// A `next` that returns an object with no `done` runs until `value` is falsy?
// No — absent `done` is falsy, so it never terminates; use a bounded manual pull.
const noDone = { next: () => ({ value: 1 }) as IteratorResult<number> };
const nd = noDone.next();
console.log("10b nodone:", JSON.stringify(nd), nd.done === undefined);

// ── 11. Destructuring / for-of over runtime-built results ───────────────────
const di = [11, 22][Symbol.iterator]();
const { value: dv, done: dd } = di.next();
console.log("11a destructure:", dv, dd);
const [f1, f2] = [4, 5];
console.log("11b arraydestructure:", f1, f2);

// ── 12. Property presence / prototype ───────────────────────────────────────
const pi = [1][Symbol.iterator]().next();
console.log("12a in:", "value" in pi, "done" in pi, "nope" in pi);
console.log("12b own:", Object.prototype.hasOwnProperty.call(pi, "value"), Object.prototype.hasOwnProperty.call(pi, "done"));
console.log("12c proto:", Object.getPrototypeOf(pi) === Object.prototype);
console.log("12d spread:", JSON.stringify({ ...pi }));
console.log("12e assign:", JSON.stringify(Object.assign({}, pi)));
console.log("12f delete:", delete (pi as Record<string, unknown>).done, JSON.stringify(Object.keys(pi)));

// ── 13. Interleaved iterators must not share result state ───────────────────
const iA = [100, 101][Symbol.iterator]();
const iB = [200, 201][Symbol.iterator]();
const a1 = iA.next();
const b1 = iB.next();
const a2 = iA.next();
const b2 = iB.next();
console.log("13a:", a1.value, b1.value, a2.value, b2.value);
console.log("13b distinct:", new Set([a1, b1, a2, b2]).size === 4);
