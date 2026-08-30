// #9180 — the receiver-based [[Set]] walk's "does the receiver already own
// this key" probe (`obj_value_has_own_key`) used a per-element `js_array_get`
// scan of the receiver's keys array. Every store that the direct-store lane
// declines re-scanned every key already installed, so building an object
// property-by-property was quadratic; it is now the shared #6759 key index
// (O(1) at/above 32 keys, raw dense-slot compare below it).
//
// This exercises BOTH tiers and, critically, the paths where the index
// declines or reports absence: below-threshold objects, an object grown
// across the 32-key threshold, a delete that shrinks the index (the
// `Unindexed` re-entry), and a re-add after the delete (the `Absent`
// completeness verdict). Plus the correctness surface of the walk itself:
// receiver !== target, proxies, accessors, non-writable data, defineProperty,
// prototype shadowing, non-extensible receivers, and index-vs-name keys.

function out(label: string, v: any): void {
  console.log(label + "=" + JSON.stringify(v));
}

// ---------------------------------------------------------------- 1. below
// the index threshold: dense raw-slot scan. Receiver has a non-default
// prototype, so the direct-store lane declines and the spec walk runs.
const base1: any = { inherited: 1 };
const small: any = Object.create(base1);
for (let i = 0; i < 8; i++) small["k" + i] = i;
small.k3 = 99;
out("small.keys", Object.keys(small));
out("small.k3", small.k3);
out("small.inherited", small.inherited);
out("small.hasOwn.inherited", Object.prototype.hasOwnProperty.call(small, "inherited"));

// -------------------------------------------------- 2. across the threshold
// 40 keys crosses KEYS_INDEX_THRESHOLD (32): the first 31 stores take the
// dense scan, the rest consult the shape index. Overwrites after the crossing
// must still find the ALREADY-PRESENT key (an `Absent` verdict here would
// silently create a duplicate or drop the update).
const base2: any = { tail: "proto" };
const wide: any = Object.create(base2);
for (let i = 0; i < 40; i++) wide["p" + i] = i;
for (let i = 0; i < 40; i += 7) wide["p" + i] = -i;
out("wide.keyCount", Object.keys(wide).length);
out("wide.p0", wide.p0);
out("wide.p7", wide.p7);
out("wide.p35", wide.p35);
out("wide.p36", wide.p36);
out("wide.tail", wide.tail);

// ------------------------------------------------- 3. delete then re-add
// A delete shrinks the keys array, which invalidates the shape index. The
// next probe must NOT trust a stale `Absent`: the re-added key has to land as
// one property, not two, and the overwrite after it has to find it.
delete wide.p10;
delete wide.p11;
out("wide.afterDelete.count", Object.keys(wide).length);
out("wide.afterDelete.p10", wide.p10);
wide.p10 = 1000;
wide.p10 = 1001;
out("wide.readd.count", Object.keys(wide).length);
out("wide.readd.p10", wide.p10);
out("wide.readd.hasOwn", Object.prototype.hasOwnProperty.call(wide, "p10"));

// ------------------------------------- 4. receiver !== target (Reflect.set)
// This is what `ordinary_set_with_receiver` exists for: the property is
// created on the RECEIVER, never on the target.
const target4: any = Object.create({ z: 0 });
const receiver4: any = Object.create({ z: 0 });
for (let i = 0; i < 34; i++) receiver4["r" + i] = i;
out("reflect.set", Reflect.set(target4, "r5", 555, receiver4));
out("reflect.receiver.r5", receiver4.r5);
out("reflect.target.r5", target4.r5);
out("reflect.target.hasOwn", Object.prototype.hasOwnProperty.call(target4, "r5"));
out("reflect.set.fresh", Reflect.set(target4, "brandNew", 7, receiver4));
out("reflect.receiver.brandNew", receiver4.brandNew);
out("reflect.target.brandNew", target4.brandNew);

// --------------------------------------------- 5. non-writable own data
// An existing non-writable own property on the receiver rejects the store.
const nw: any = Object.create({ q: 0 });
for (let i = 0; i < 33; i++) nw["n" + i] = i;
Object.defineProperty(nw, "locked", { value: 1, writable: false, enumerable: true, configurable: true });
out("nonwritable.reflect", Reflect.set(nw, "locked", 2));
out("nonwritable.value", nw.locked);
out("nonwritable.stillFindsOthers", Reflect.set(nw, "n7", 77));
out("nonwritable.n7", nw.n7);

// --------------------------------------------------- 6. accessor on the
// prototype: the setter fires and no own property is created.
let sawSetter: any = null;
const accProto: any = {};
Object.defineProperty(accProto, "acc", {
  get() { return sawSetter; },
  set(v: any) { sawSetter = v; },
  configurable: true,
});
const accObj: any = Object.create(accProto);
for (let i = 0; i < 33; i++) accObj["a" + i] = i;
accObj.acc = "viaSetter";
out("accessor.value", accObj.acc);
out("accessor.hasOwn", Object.prototype.hasOwnProperty.call(accObj, "acc"));
out("accessor.sawSetter", sawSetter);

// ------------------------------------------- 7. own accessor on receiver
// An own accessor on the RECEIVER makes the CreateDataProperty tail return
// false without invoking the setter (OrdinarySetWithOwnDescriptor 2.d.i).
let receiverSetterCalls = 0;
const t7: any = Object.create({ w: 0 });
const r7: any = Object.create({ w: 0 });
for (let i = 0; i < 33; i++) r7["s" + i] = i;
Object.defineProperty(r7, "own", {
  get() { return "g"; },
  set(_v: any) { receiverSetterCalls++; },
  configurable: true,
});
out("receiverAccessor.reflect", Reflect.set(t7, "own", 5, r7));
out("receiverAccessor.calls", receiverSetterCalls);
out("receiverAccessor.value", r7.own);

// ----------------------------------------------------- 8. non-extensible
const sealedObj: any = Object.create({ v: 0 });
for (let i = 0; i < 33; i++) sealedObj["e" + i] = i;
Object.preventExtensions(sealedObj);
out("nonextensible.existing", Reflect.set(sealedObj, "e4", 44));
out("nonextensible.e4", sealedObj.e4);
out("nonextensible.new", Reflect.set(sealedObj, "brandNew", 1));
out("nonextensible.brandNew", sealedObj.brandNew);

// ------------------------------------------ 9. index-like vs name keys
// Canonical integer-index STRING keys and their numeric twins are the same
// property; a leading-zero form is a distinct ordinary name.
const idx: any = Object.create({ y: 0 });
for (let i = 0; i < 33; i++) idx["i" + i] = i;
idx[2] = "two";
idx["2"] = "TWO";
idx["02"] = "ohtwo";
out("index.2", idx[2]);
out("index.str2", idx["2"]);
out("index.02", idx["02"]);
out("index.count", Object.keys(idx).length);

// ------------------------------------------------------------ 10. proxy
// A proxy RECEIVER routes the tail through [[DefineOwnProperty]].
const proxyTarget: any = Object.create({ pp: 0 });
for (let i = 0; i < 33; i++) proxyTarget["x" + i] = i;
const trapLog: string[] = [];
const proxied: any = new Proxy(proxyTarget, {
  defineProperty(t: any, k: any, d: any) {
    trapLog.push("define:" + String(k));
    return Reflect.defineProperty(t, k, d);
  },
  set(t: any, k: any, v: any, r: any) {
    trapLog.push("set:" + String(k));
    return Reflect.set(t, k, v, r);
  },
});
proxied.x4 = 444;
proxied.newOne = 1;
out("proxy.x4", proxyTarget.x4);
out("proxy.newOne", proxyTarget.newOne);
out("proxy.trapLog", trapLog);

// ----------------------------------- 11. prototype-chain shadowing order
const shadowProto: any = { shared: "proto" };
const shadowObj: any = Object.create(shadowProto);
for (let i = 0; i < 33; i++) shadowObj["h" + i] = i;
out("shadow.before", shadowObj.shared);
shadowObj.shared = "own";
out("shadow.after", shadowObj.shared);
out("shadow.proto", shadowProto.shared);
out("shadow.hasOwn", Object.prototype.hasOwnProperty.call(shadowObj, "shared"));

// ------------------------------- 12. quadratic shape: many distinct keys
// The scan this replaces was O(own-key-count) per store. Answer must be
// exact, not merely fast.
const big: any = Object.create({ tailKey: "t" });
for (let i = 0; i < 300; i++) big["b" + i] = i;
let sum = 0;
for (let i = 0; i < 300; i++) sum += big["b" + i];
out("big.count", Object.keys(big).length);
out("big.sum", sum);
out("big.first", big.b0);
out("big.last", big.b299);
out("big.absent", big.b300);
out("big.tailKey", big.tailKey);
