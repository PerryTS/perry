// `field_count` is the number of properties resident in an object's INLINE slots
// — every reader treats `field_index >= field_count` as living in the overflow
// map. It is NOT the property count: an object with 9 properties and 8 inline
// slots carries `field_count == 8`, with the 9th spilled to overflow.
//
// `delete` decremented it by one regardless. Deleting one property from that
// 9-property object leaves 8 survivors — all of which now FIT inline — but
// `field_count` became 7, so the last survivor's slot (index 7) sat at the
// boundary and was read from the overflow map, which held nothing. Its key stayed
// enumerable while its value vanished:
//
//   Object.keys(o)      -> still lists it
//   o.keep              -> undefined
//   JSON.stringify(o)   -> drops it entirely
//
// It bites any object that ever grew past 8 properties. Next.js deletes a batch of
// `x-middleware-request-*` keys from the middleware's header object; the surviving
// `x-middleware-rewrite` came back `undefined`, so Next stopped rewriting to the
// page and served an empty body.

const o: any = {};
for (let i = 0; i < 8; i++) o["k" + i] = "v" + i;
o["keep"] = "KEEP";

console.log("before      :", o["keep"], Object.keys(o).length);
delete o["k0"];
console.log("after       :", o["keep"]);
console.log("keys        :", Object.keys(o).join(","));
console.log("json        :", JSON.stringify(o));

// exhaustive: build n properties, delete an arbitrary subset, compare with a Map
function check(n: number, deleteIdxs: number[]): boolean {
  const obj: any = {};
  const model = new Map<string, string>();
  for (let i = 0; i < n; i++) {
    obj["k" + i] = "v" + i;
    model.set("k" + i, "v" + i);
  }
  for (const i of deleteIdxs) {
    delete obj["k" + i];
    model.delete("k" + i);
  }
  if (Object.keys(obj).join(",") !== [...model.keys()].join(",")) return false;
  for (const [k, v] of model) if (obj[k] !== v) return false;
  if (JSON.stringify(obj) !== JSON.stringify(Object.fromEntries(model))) return false;
  if (JSON.stringify(Object.entries(obj)) !== JSON.stringify([...model])) return false;
  return true;
}

let failures = 0;
let cases = 0;
for (let n = 1; n <= 16; n++) {
  const bits = Math.min(n, 6);
  for (let mask = 0; mask < 1 << bits; mask++) {
    const del: number[] = [];
    for (let b = 0; b < bits; b++) if (mask & (1 << b)) del.push(b);
    cases++;
    if (!check(n, del)) failures++;
  }
}
console.log("exhaustive  :", cases, "cases,", failures, "failures");

// re-adding after a delete, and every enumeration path agreeing
const r: any = {};
for (let i = 0; i < 10; i++) r["k" + i] = i;
delete r["k3"];
delete r["k7"];
r["k3"] = "re-added";
r["fresh"] = "F";
console.log("re-add      :", r["k3"], r["k7"], r["fresh"], r["k9"]);
const seen: string[] = [];
for (const k in r) seen.push(k);
console.log("for-in      :", seen.join(","));
console.log("spread      :", JSON.stringify({ ...r }));

// an accessor must survive a delete on the same object — it must NOT be
// collapsed into a data property (Next's module exports are defineProperty
// getters, and flattening them breaks every route)
const a: any = {};
Object.defineProperty(a, "lazy", {
  get() {
    return "COMPUTED:" + (a.n || 0);
  },
  enumerable: true,
  configurable: true,
});
for (let i = 0; i < 9; i++) a["k" + i] = i;
a.n = 7;
console.log("accessor    :", a.lazy);
delete a["k0"];
a.n = 8;
console.log("after delete:", a.lazy);
const d = Object.getOwnPropertyDescriptor(a, "lazy")!;
console.log("still getter:", typeof d.get === "function", d.value === undefined);
