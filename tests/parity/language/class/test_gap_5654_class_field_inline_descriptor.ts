// #5654: the #5093 codegen-inlined class-field fast path must stay enabled for
// unrelated descriptor installs (they used to disable it process-wide — so it
// never engaged in ordinary programs), while descriptors that actually target a
// class instance or a class prototype still intercept `this.field` accesses.
//
// Scenarios:
//   1. warm inline fast path, then install an accessor on THE INSTANCE —
//      subsequent reads/writes must route through the accessor (per-object
//      OBJ_FLAG_HAS_DESCRIPTORS rejects the inline path for that receiver);
//   2. non-writable data descriptor on the instance — writes must throw
//      (strict mode) and leave the value unchanged;
//   3. accessor on the CLASS PROTOTYPE (flips the process-wide gate) — the
//      own field defined by the class body shadows it (strip-types leaves
//      `p;`, an own-property define), so reads/writes keep hitting the own
//      slot: both engines must agree the inherited accessor does NOT fire;
//   4. unrelated descriptors (plain object defineProperty / freeze) must not
//      disturb class-field behavior on untouched instances.

class Box {
    v: number;
    constructor(x: number) {
        this.v = x;
    }
    bump(): number {
        this.v = this.v + 1;
        return this.v;
    }
}

// ── 1. accessor installed directly on a warmed instance ────────────────────
const a = new Box(0);
for (let i = 0; i < 2000; i++) a.bump();
console.log("warmed:" + a.v); // 2000

let backing = 500;
let getCount = 0;
let setCount = 0;
Object.defineProperty(a, "v", {
    get() {
        getCount++;
        return backing;
    },
    set(x: number) {
        setCount++;
        backing = x;
    },
    configurable: true,
});
console.log("acc-read:" + a.v); // 500
a.v = 7;
console.log("acc-write:" + backing); // 7
a.bump(); // method path: get + set through the accessor
console.log("acc-bump:" + backing); // 8
console.log("counts:" + (getCount >= 2) + ":" + (setCount >= 2)); // true:true

// ── 2. non-writable data descriptor on another warmed instance ─────────────
const b = new Box(10);
for (let i = 0; i < 2000; i++) b.bump();
Object.defineProperty(b, "v", { value: 123, writable: false });
let threw = false;
try {
    b.v = 999;
} catch (e) {
    threw = true;
}
console.log("ro-threw:" + threw); // true (module code is strict)
console.log("ro-value:" + b.v); // 123

// ── 3. accessor on the class prototype intercepts later instances ──────────
class Probe {
    p: number;
    constructor(x: number) {
        this.p = x;
    }
}
const before = new Probe(1);
console.log("proto-before:" + before.p); // 1

let protoBacking = -1;
Object.defineProperty(Probe.prototype, "p", {
    get() {
        return protoBacking;
    },
    set(x: number) {
        protoBacking = x * 10;
    },
    configurable: true,
});
// The class body's `p;` field define creates an own `p` before the ctor body
// runs, so `this.p = 4` writes the own slot — the inherited setter must NOT
// fire (own data property shadows the prototype accessor).
const after = new Probe(4);
console.log("proto-backing:" + protoBacking); // -1
console.log("proto-read:" + after.p); // 4

// ── 4. unrelated descriptors leave untouched instances alone ───────────────
const plain: { k: number } = { k: 1 };
Object.defineProperty(plain, "k", { value: 2, writable: false });
Object.freeze({ other: true });

const c = new Box(100);
let sum = 0;
for (let i = 0; i < 2000; i++) sum += c.bump();
console.log("clean-v:" + c.v); // 2100
console.log("clean-sum:" + sum); // sum of 101..2100
console.log("plain-k:" + plain.k); // 2
