// #5093: class-field versioned loop — the call-free fast clone must preserve
// exact JS semantics across the preheader shape check, the per-store
// plain-finite side exit, and every slow-clone fallback.

// 1) The canonical hot loop (method inlined -> counter.value = counter.value + 1).
class Counter {
    value: number;
    constructor() {
        this.value = 0;
    }
    increment(): void {
        this.value = this.value + 1;
    }
}
const counter = new Counter();
for (let i = 0; i < 100000; i++) {
    counter.increment();
}
console.log("hot:" + counter.value);

// 2) Field initialized at declaration + direct store in the loop body.
class Cell {
    v: number = 5;
}
const cell = new Cell();
for (let i = 0; i < 1000; i++) {
    cell.v = cell.v + 2;
}
console.log("decl-init:" + cell.v);

// 3) Loop-invariant module-const bound + multiple reads per iteration.
const N = 2048;
class Pair {
    a: number = 1;
    b: number = 2;
}
const pair = new Pair();
for (let i = 0; i < N; i++) {
    pair.a = pair.a + pair.b;
}
console.log("pair:" + pair.a + "," + pair.b);

// 4) Mid-loop overflow to Infinity: the raw store's plain-finite check must
// side-exit to the slow clone WITHOUT committing, and the slow clone must
// finish the remaining iterations with identical semantics.
class Grow {
    x: number = 1e308;
}
const grow = new Grow();
for (let i = 0; i < 8; i++) {
    grow.x = grow.x * 2;
}
console.log("overflow:" + grow.x);

// 5) Subclass instance held in a base-typed binding: the preheader class_id
// check must reject it (subclass has its own class id) and the slow clone
// must produce correct results, including the shadowing subclass field order.
class Base {
    n: number = 0;
}
class Derived extends Base {
    extra: number = 7;
}
const asBase: Base = new Derived();
for (let i = 0; i < 500; i++) {
    asBase.n = asBase.n + 1;
}
console.log("subclass:" + asBase.n + "," + (asBase as Derived).extra);

// 6) Frozen receiver: strict-mode store to a frozen object throws TypeError.
// The preheader's not-frozen check routes the loop to the slow clone, which
// must throw exactly like an unversioned loop.
class Frost {
    f: number = 3;
}
const frost = new Frost();
Object.freeze(frost);
let frozenError = "none";
try {
    for (let i = 0; i < 10; i++) {
        frost.f = frost.f + 1;
    }
} catch (e) {
    frozenError = (e as Error).constructor.name;
}
console.log("frozen:" + frozenError + "," + frost.f);

// 7) defineProperty on a class instance: installs a descriptor on a
// class-relevant target, which must disable the inline fast path globally and
// keep accessor semantics through the loop.
class Acc {
    y: number = 10;
}
const acc = new Acc();
let getterHits = 0;
Object.defineProperty(acc, "y", {
    get() {
        getterHits = getterHits + 1;
        return 42;
    },
    configurable: true,
});
let sum = 0;
for (let i = 0; i < 5; i++) {
    sum = sum + acc.y;
}
console.log("accessor:" + sum + "," + (getterHits > 0 ? "hit" : "miss"));

// 8) Accumulator-only loop (no field store): reads must stay raw and the
// result must match plain evaluation.
class Score {
    s: number = 1.5;
}
const score = new Score();
function tally(): number {
    let acc2 = 0;
    for (let i = 0; i < 4000; i++) {
        acc2 = acc2 + score.s;
    }
    return acc2;
}
console.log("tally:" + tally());
