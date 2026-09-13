// Every arm of the generic property-get tower, against node's own output.
//
// `lower_generic_property_get` used to expand each of these as its own inline
// block group with its own runtime call: the SSO receiver, the heap-string
// receiver, the INT32 class ref, the nullish TypeError, the non-object
// receiver, the native Map/Set `.size`, the overflow slot, the deleted slot,
// and the Array-subclass named-prefix proof. They now live behind two runtime
// entries (`js_object_get_field_ic_nonptr` for a receiver that is not a heap
// pointer, `js_object_get_field_ic_slow` for one that is), so a defect in any
// of them is no longer a defect in emitted code that some other test might
// notice — it is a defect in one shared function, and this is the witness that
// each arm still answers what node answers.
//
// Every read below goes through `read()`, whose parameter is `any`: that is
// what denies the front end a type proof and lands the access in the generic
// tower rather than in one of the specialised lowerings.

function read(o: any, k: string): any {
    // One generic `obj.prop` site per arm.
    if (k === "a") return o.a;
    if (k === "length") return o.length;
    if (k === "size") return o.size;
    if (k === "kind") return o.kind;
    if (k === "charCodeAt") return o.charCodeAt;
    return o.zzz;
}

// --- receiver tags that are not heap pointers -------------------------------

// SSO string (short enough to live inside the NaN box) and a heap string.
const sso: any = "hi";
const heap: any = "a considerably longer string";
console.log("sso.length", read(sso, "length"));
console.log("heap.length", read(heap, "length"));
console.log("sso.charCodeAt is fn", typeof read(sso, "charCodeAt"));
console.log("heap.zzz", read(heap, "zzz"));

// A class object read as a value: its static field, not an instance field.
class C {
    static kind = "static-c";
    x = 1;
}
const cref: any = C;
console.log("C.kind", read(cref, "kind"));

// Non-nullish primitives: no auto-boxing, no throw, just `undefined`.
console.log("3.5.zzz", read(3.5 as any, "zzz"));
console.log("true.zzz", read(true as any, "zzz"));

// --- heap receivers that are not ordinary objects ---------------------------

const m: any = new Map();
m.set(1, 2);
const st: any = new Set();
st.add(9);
console.log("map.size", read(m, "size"), "set.size", read(st, "size"));

const arr: any = [1, 2, 3];
console.log("arr.length", read(arr, "length"));

// --- ordinary objects: hit, hole, re-add ------------------------------------

const o: any = { a: 1, b: 2 };
console.log("o.a", read(o, "a"));
delete o.a;
console.log("o.a deleted", read(o, "a"));
o.a = 7;
console.log("o.a again", read(o, "a"));

// A field past the inline region (the overflow slot), read repeatedly so the
// site primes and then takes the primed path, then tombstoned.
const big: any = {};
big.p0 = 0;
big.p1 = 1;
big.p2 = 2;
big.p3 = 3;
big.p4 = 4;
for (let i = 0; i < 3; i++) console.log("big.p4", big.p4);
delete big.p4;
console.log("big.p4 deleted", big.p4);

// Absent own key: the prototype chain answers.
const proto = { inherited: "yes" };
const child: any = Object.create(proto);
console.log("child.inherited", child.inherited);

// A descriptor-bearing receiver: the accessor must fire on EVERY read, which
// is what forbids a raw-slot hit from ever being primed for it.
let calls = 0;
const acc: any = {};
Object.defineProperty(acc, "g", {
    get() {
        calls += 1;
        return calls;
    },
});
console.log("acc.g", acc.g, acc.g, acc.g, "calls", calls);

// --- nullish receivers throw a node-shaped TypeError ------------------------

try {
    const n: any = null;
    console.log(n.foo);
} catch (e: any) {
    console.log("caught null:", e.message);
}
try {
    const u: any = undefined;
    console.log(u.bar);
} catch (e: any) {
    console.log("caught undefined:", e.message);
}

// --- shape rotation: the bounded polymorphic ways ---------------------------

class S1 {
    x = 1;
}
class S2 {
    a = 0;
    x = 2;
}
class S3 {
    a = 0;
    b = 0;
    x = 3;
}
const rot: any[] = [new S1(), new S2(), new S3()];
let sum = 0;
for (let i = 0; i < 300; i++) sum += rot[i % rot.length].x;
console.log("poly sum", sum);
