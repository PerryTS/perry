// Dynamic-key property reads, `o[k]`, across the receiver shapes the fast path
// in `native_get::try_data_get_bytes` classifies — and the mutations that must
// invalidate what it reads.
//
// Three per-access re-derivations were removed behind this: the keys array was
// re-resolved through `clean_arr_ptr` although the collector maintains
// `ShapeDescriptor::keys`; a thread-local was read to answer "is this
// process.env?"; and an RwLock read guard was taken to ask whether the
// receiver's class id is an anon shape. The last is answered from a per-IMAGE
// lock-free mirror, so anything that depends on the anon-shape verdict
// (`.constructor`, `Object.getPrototypeOf`) is exercised here too.
const plain: any = { a: 1, b: "two", c: true };
for (const k of ["a", "b", "c", "missing"]) console.log("plain", k, String(plain[k]));

// Anon shape: an object literal's `.constructor` must still be Object, which
// is the verdict the mirror now answers.
console.log("ctor is Object:", plain.constructor === Object);
console.log("proto is Object.prototype:", Object.getPrototypeOf(plain) === Object.prototype);

// Key added after first read: the shape changes, so the cached keys array must
// not be reused.
const grown: any = { x: 1 };
console.log("before add", String(grown.y));
grown.y = 42;
console.log("after add", String(grown.y));

// Key deleted after first read.
const shrunk: any = { p: 1, q: 2 };
console.log("before delete", String(shrunk.q));
delete shrunk.q;
console.log("after delete", String(shrunk.q));

// Many keys, to cross the indexed-lookup threshold the resolved entry
// deliberately delegates back to the original path for.
const wide: any = {};
for (let i = 0; i < 64; i++) wide["k" + i] = i;
let sum = 0;
for (let i = 0; i < 64; i++) sum += wide["k" + i];
console.log("wide sum", sum);

// A declared class instance is NOT an anon shape.
class Holder { v: number; constructor(v: number) { this.v = v; } }
const inst: any = new Holder(7);
console.log("class read", inst["v"], "ctor", inst.constructor === Holder);

// A prototype-chain read through a dynamic key.
const parent: any = { inherited: "yes" };
const child: any = Object.create(parent);
child.own = "mine";
console.log("own", child["own"], "inherited", child["inherited"]);

// Accessors must not be answered from the data fast path.
const acc: any = { get computed() { return "from-getter"; } };
console.log("accessor", acc["computed"]);

// Hot loop over one key, which is the shape the removals were measured on.
const hot: any = { k: 1.5, other: 2 };
let t = 0;
for (let i = 0; i < 500; i++) t += hot["k"];
console.log("hot total", t);
