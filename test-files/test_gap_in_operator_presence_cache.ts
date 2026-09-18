// A warm `"k" in o` site caches "this shape has own key k". Every way of
// losing the key — a compacting delete, a tombstoning delete, a re-add, a
// shape change — must be visible at the very next evaluation of the SAME
// site, and a prototype mutation must be visible even though the cached claim
// is only about own keys. Each helper below is one call site, deliberately
// reused so the invalidation is proven against a cache that is already warm.

function hasA(o: any): boolean {
  return "a" in o;
}
function hasP(o: any): boolean {
  return "p" in o;
}
function hasToString(o: any): boolean {
  return "toString" in o;
}

// ---- warm, then delete through the same site -------------------------------
const warm: any = { a: 1, b: 2 };
let seen = 0;
for (let i = 0; i < 2000; i++) if (hasA(warm)) seen++;
console.log("warm", seen, hasA(warm));
delete warm.a;
console.log("deleted", hasA(warm), "a" in warm, warm.a);
warm.a = 9;
console.log("re-added", hasA(warm), warm.a);

// ---- delete and re-add repeatedly through the warm site ---------------------
const churn: any = { a: 1, b: 2, c: 3 };
let churnTrue = 0;
let churnFalse = 0;
for (let i = 0; i < 500; i++) {
  if (hasA(churn)) churnTrue++;
  delete churn.a;
  if (hasA(churn)) churnFalse++;
  churn.a = i;
}
console.log("churn", churnTrue, churnFalse, hasA(churn));

// ---- a warm site, then the prototype moves under the receiver ---------------
const protoA = { p: 1 };
const protoB = { q: 2 };
const moving: any = Object.create(protoA);
moving.own = 1;
let protoSeen = 0;
for (let i = 0; i < 2000; i++) if (hasP(moving)) protoSeen++;
console.log("proto-warm", protoSeen, hasP(moving));
Object.setPrototypeOf(moving, protoB);
console.log("proto-swapped", hasP(moving), "q" in moving);
Object.setPrototypeOf(moving, null);
console.log("proto-null", hasP(moving), hasToString(moving), "own" in moving);
Object.setPrototypeOf(moving, protoA);
console.log("proto-restored", hasP(moving));

// ---- the key appears and disappears ON the prototype after a warm hit -------
const lateProto: any = {};
const lateChild: any = Object.create(lateProto);
lateChild.own = 1;
let lateSeen = 0;
for (let i = 0; i < 2000; i++) if (hasP(lateChild)) lateSeen++;
console.log("late-before", lateSeen, hasP(lateChild));
lateProto.p = 7;
console.log("late-added", hasP(lateChild));
delete lateProto.p;
console.log("late-removed", hasP(lateChild));

// ---- an own key that shadows, then is deleted so the proto shows through ----
const shadowProto: any = { a: "proto" };
const shadow: any = Object.create(shadowProto);
shadow.a = "own";
let shadowSeen = 0;
for (let i = 0; i < 2000; i++) if (hasA(shadow)) shadowSeen++;
console.log("shadow-warm", shadowSeen, shadow.a);
delete shadow.a;
console.log("shadow-deleted", hasA(shadow), shadow.a);
delete shadowProto.a;
console.log("shadow-proto-deleted", hasA(shadow), shadow.a);

// ---- one site, many shapes -------------------------------------------------
const shapes: any[] = [
  { a: 1 },
  { a: 1, b: 2 },
  { b: 2 },
  { x: 0, y: 0, a: 3 },
  Object.create({ a: "inherited" }),
  { get a() {
      return 1;
    } },
  Object.freeze({ a: 1 }),
  Object.seal({ a: 1, z: 2 }),
];
let polyCount = 0;
for (let i = 0; i < 2000; i++) if (hasA(shapes[i % shapes.length])) polyCount++;
console.log("poly", polyCount, shapes.map((s) => hasA(s)).join(","));

// ---- descriptors and non-enumerables ---------------------------------------
const desc: any = {};
Object.defineProperty(desc, "a", { value: 1, enumerable: false, configurable: true });
let descSeen = 0;
for (let i = 0; i < 1000; i++) if (hasA(desc)) descSeen++;
console.log("descriptor", descSeen, hasA(desc));
delete desc.a;
console.log("descriptor-deleted", hasA(desc));

const accessor: any = {};
Object.defineProperty(accessor, "a", { get: () => 1, configurable: true });
let accSeen = 0;
for (let i = 0; i < 1000; i++) if (hasA(accessor)) accSeen++;
console.log("accessor", accSeen, hasA(accessor));
delete accessor.a;
console.log("accessor-deleted", hasA(accessor));

// ---- a class instance, and a method that is not an own key -----------------
class Point {
  a = 1;
  b = 2;
  moveIt(): number {
    return this.a;
  }
}
const pt: any = new Point();
let ptSeen = 0;
for (let i = 0; i < 2000; i++) if (hasA(pt)) ptSeen++;
console.log("class", ptSeen, hasA(pt), "moveIt" in pt, "nope" in pt);
delete pt.a;
console.log("class-deleted", hasA(pt), "b" in pt);

// ---- a wide object, across the keys-index threshold -------------------------
const wide: any = {};
for (let i = 0; i < 40; i++) wide["k" + i] = i;
wide.a = "wide";
let wideSeen = 0;
for (let i = 0; i < 2000; i++) if (hasA(wide)) wideSeen++;
console.log("wide", wideSeen, hasA(wide), "k39" in wide, "k40" in wide);
delete wide.a;
console.log("wide-deleted", hasA(wide), "k39" in wide);

// ---- non-object receivers through the same warm site -----------------------
const proxy: any = new Proxy({ a: 1 }, {
  has(t, k) {
    return k === "a" ? false : k in t;
  },
});
let proxySeen = 0;
for (let i = 0; i < 1000; i++) if (hasA(proxy)) proxySeen++;
console.log("proxy", proxySeen, hasA(proxy), "b" in proxy);

const arr: any = [1, 2, 3];
(arr as any).a = 1;
console.log("array", hasA(arr), "0" in arr, "3" in arr, "length" in arr);
console.log("builtins", hasA(new Map()), hasA(new Set()), hasA(/re/), "lastIndex" in /re/);
console.log("wrapper", hasA(new String("xy")), "0" in new String("xy"));

// ---- a primitive right operand still throws --------------------------------
let threw = "";
try {
  hasA(5 as any);
} catch (e) {
  threw = (e as Error).constructor.name;
}
console.log("primitive", threw);
let threwNull = "";
try {
  hasA(null as any);
} catch (e) {
  threwNull = (e as Error).constructor.name;
}
console.log("null", threwNull);

// ---- hot loop where the delete happens INSIDE the loop ----------------------
const inLoop: any = { a: 1, b: 2 };
let hits = 0;
for (let i = 0; i < 1000; i++) {
  if (hasA(inLoop)) hits++;
  if (i === 500) delete inLoop.a;
}
console.log("in-loop", hits, hasA(inLoop));

// ---- and one where the object itself is replaced each iteration -------------
let fresh = 0;
for (let i = 0; i < 1000; i++) {
  const o: any = i % 2 === 0 ? { a: i } : { b: i };
  if (hasA(o)) fresh++;
}
console.log("fresh", fresh);
