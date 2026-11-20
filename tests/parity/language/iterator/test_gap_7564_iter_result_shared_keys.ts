// #7564 copy-on-write matrix. Every runtime-built iterator result on a thread
// now SHARES one keys array (stamped `GC_FLAG_SHAPE_SHARED`), which is what
// removes four of the five per-`.next()` allocations and collapses the shape
// table to one entry. The entire soundness argument is that no path can reach
// through a result object and mutate that array: `field_set_by_name`,
// `delete_rest` and `proxy::put_value` each clone before writing.
//
// If any of them stopped consulting the flag, the failure would not be local.
// One `result.extra = 1` would append a third key to the array EVERY other
// iterator result is using, so results produced BEFORE the mutation and
// results produced AFTER it would all silently grow a key. That is what these
// cases check: the sibling results, not just the mutated one.

// ── 1. Add a property, then check a long run of later results ───────────────
const a1 = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9][Symbol.iterator]();
const first = a1.next() as Record<string, unknown>;
first.injected = "x";
let laterBad = 0;
for (let i = 0; i < 9; i++) {
    const r = a1.next() as Record<string, unknown>;
    if (Object.keys(r).join(",") !== "value,done") laterBad++;
    if (r.injected !== undefined) laterBad++;
}
console.log("1a laterBad:", laterBad);
console.log("1b mutated:", JSON.stringify(Object.keys(first)));

// ── 2. Results captured BEFORE the mutation must not grow a key either ──────
const a2 = ["p", "q", "r"][Symbol.iterator]();
const before = [a2.next(), a2.next()];
const victim = a2.next() as Record<string, unknown>;
victim.late = 1;
console.log("2a before:", before.map((b) => Object.keys(b).join("|")).join(" "));
console.log("2b victim:", JSON.stringify(Object.keys(victim)));

// ── 3. defineProperty is a different write path than plain assignment ───────
const a3 = [10, 20, 30][Symbol.iterator]();
const d1 = a3.next();
Object.defineProperty(d1, "hidden", { value: 7, enumerable: false, configurable: true });
const d2 = a3.next();
console.log("3a d1keys:", JSON.stringify(Object.keys(d1)));
console.log("3b d1has:", Object.prototype.hasOwnProperty.call(d1, "hidden"));
console.log("3c d2keys:", JSON.stringify(Object.keys(d2)));
console.log("3d d2has:", Object.prototype.hasOwnProperty.call(d2, "hidden"));

// enumerable defineProperty
const a4 = [1, 2, 3][Symbol.iterator]();
const e1 = a4.next();
Object.defineProperty(e1, "shown", { value: 8, enumerable: true, configurable: true });
const e2 = a4.next();
console.log("3e e1keys:", JSON.stringify(Object.keys(e1)));
console.log("3f e2keys:", JSON.stringify(Object.keys(e2)));

// ── 4. delete clones too — a shortened result must not shorten its siblings ─
const a5 = [4, 5, 6, 7][Symbol.iterator]();
const del1 = a5.next() as Record<string, unknown>;
delete del1.done;
const del2 = a5.next();
const del3 = a5.next();
console.log("4a del1:", JSON.stringify(Object.keys(del1)));
console.log("4b del2:", JSON.stringify(Object.keys(del2)));
console.log("4c del3:", JSON.stringify(Object.keys(del3)), JSON.stringify(del3));

// ── 5. freeze / seal must not leak to siblings ──────────────────────────────
const a6 = [1, 2, 3][Symbol.iterator]();
const fr = a6.next() as Record<string, unknown>;
Object.freeze(fr);
const notFrozen = a6.next() as Record<string, unknown>;
notFrozen.ok = 1;
console.log("5a frozen:", Object.isFrozen(fr), Object.isFrozen(notFrozen));
console.log("5b sibling:", JSON.stringify(Object.keys(notFrozen)));

// ── 6. Different iterator KINDS share the same array — cross-kind check ─────
// A property added to a Map-iterator result must not appear on a subsequent
// array-iterator or string-iterator result.
const m6 = new Map<string, number>([["k", 1], ["l", 2]]);
const mi = m6.values();
const mr = mi.next() as Record<string, unknown>;
mr.crosskind = true;
const arrR = [99][Symbol.iterator]().next();
const strR = "z"[Symbol.iterator]().next();
const bufR = new Uint8Array([3]).entries().next();
console.log("6a arr:", JSON.stringify(Object.keys(arrR)), JSON.stringify(arrR));
console.log("6b str:", JSON.stringify(Object.keys(strR)), JSON.stringify(strR));
console.log("6c buf:", JSON.stringify(Object.keys(bufR)), JSON.stringify(bufR));
console.log("6d gen:", JSON.stringify(Object.keys(mr)));

// ── 7. Proxy over a result object ───────────────────────────────────────────
const a7 = [1, 2][Symbol.iterator]();
const target = a7.next() as Record<string, unknown>;
const prox = new Proxy(target, {});
prox.viaProxy = 5;
const sibling7 = a7.next();
console.log("7a target:", JSON.stringify(Object.keys(target)));
console.log("7b sibling:", JSON.stringify(Object.keys(sibling7)));

// ── 8. Volume: mutate every other result, verify the untouched ones ─────────
const a8 = [] as number[];
for (let i = 0; i < 200; i++) a8.push(i);
const it8 = a8[Symbol.iterator]();
let clean = 0;
let dirty = 0;
for (let i = 0; i < 200; i++) {
    const r = it8.next() as Record<string, unknown>;
    if (i % 2 === 0) {
        r["k" + i] = i;
        if (Object.keys(r).length === 3) dirty++;
    } else if (Object.keys(r).join(",") === "value,done") {
        clean++;
    }
}
console.log("8a clean:", clean, "dirty:", dirty);

// ── 9. Shape stability: reads keep working after heavy mutation ─────────────
const it9 = [1, 2, 3, 4, 5][Symbol.iterator]();
let sum9 = 0;
for (let i = 0; i < 5; i++) {
    const r = it9.next() as Record<string, unknown>;
    r["p" + i] = i;
    sum9 += r.value as number;
}
const after9 = [7, 8][Symbol.iterator]();
const r9 = after9.next();
console.log("9a sum:", sum9, "after:", r9.value, r9.done, JSON.stringify(Object.keys(r9)));
