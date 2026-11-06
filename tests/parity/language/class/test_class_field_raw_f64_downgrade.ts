// #5093: the codegen-inlined class-field shape guard reads a `number`-typed
// (raw-f64) field directly as a raw double when the per-object typed-layout
// intact bit is set. This test exercises the downgrade trap: writing a
// non-number into a `number`-typed slot through an `any` alias must clear that
// bit so a subsequent read returns the boxed value, never the pointer bits
// reinterpreted as a double.

class Box {
    v: number;
    constructor(x: number) {
        this.v = x;
    }
}

const b = new Box(42);

// Warm the raw-f64 fast path (read + write) so any LICM-hoisted shape check is
// established before the downgrade.
let acc = 0;
for (let i = 0; i < 1000; i++) {
    b.v = b.v + 1;
    acc = acc + b.v;
}
console.log("after-loop-v:" + b.v); // 1042
console.log("acc:" + acc);

// Downgrade: store a non-number through an `any` alias. The boxed setter must
// run, the slot must stop being raw-f64, and the intact bit must clear.
const a: any = b;
a.v = "hello";
console.log("downgraded-v:" + b.v); // hello  (NOT a garbage number)
console.log("typeof:" + typeof b.v); // string

// Re-promote: storing a number again must read back correctly either way.
b.v = 7;
console.log("repromoted-v:" + b.v); // 7
console.log("repromoted-typeof:" + typeof b.v); // number

// A fresh instance must still take the fast path correctly and independently.
const c = new Box(100);
console.log("fresh-v:" + c.v); // 100
console.log("orig-after-fresh:" + b.v); // 7

// Store null / object through the alias too — both are non-numbers that must
// downgrade safely.
const d = new Box(5);
const da: any = d;
da.v = null;
console.log("null-v:" + d.v); // null
da.v = { tag: "obj" };
console.log("obj-v:" + d.v.tag); // obj
