// A coloured key stored through receivers whose layouts hold it in a numeric
// (F64) lane or an Any lane: plain doubles, then values that widen the lane,
// then setters, non-writable, integrity levels, holes and key order.
const out: string[] = [];
const log = (...a: any[]) => out.push(a.map((x) => (typeof x === "string" ? x : String(x))).join(" "));

function setValue(o: any, v: any) {
  try {
    o.value = v;
  } catch (e) {
    log("threw", (e as Error).constructor.name);
  }
}
function getValue(o: any) {
  return o.value;
}
function setNext(o: any, v: any) {
  try {
    o.next = v;
  } catch (e) {
    log("threw", (e as Error).constructor.name);
  }
}
function getNext(o: any) {
  return o.next;
}

class Num {
  value = 0;
  next = 0;
  tag = "n";
}
class Any {
  value: any = null;
  next: any = null;
  tag = "a";
}
class Late {
  tag = "l";
  constructor() {
    this.value = 1;
  }
  value: any;
}

// 1. plain doubles into numeric lanes, -0, NaN, Infinity, subnormal, int32-like
const n = new Num();
for (const v of [1, 2.5, -0, NaN, Infinity, -Infinity, 5e-324, 2147483647, 4294967296]) {
  setValue(n, v);
  log("num", Object.is(getValue(n), v), getValue(n));
}
// 2. values that widen a numeric lane
for (const v of ["s", {}, null, undefined, true, 7n as any, [1], () => 1]) {
  const m = new Num();
  setValue(m, 3);
  setValue(m, v);
  log("widen", typeof getValue(m), String(getValue(m)), Object.keys(m).join(","));
  setValue(m, 9);
  log("back", getValue(m));
}
// 3. Any lanes take everything
const a = new Any();
for (const v of [1, "x", { z: 1 }, null, undefined, 2.5]) {
  setValue(a, v);
  log("any", typeof getValue(a));
}
// 4. setter on the prototype of a literal that lacks the own key
const protoSet = {
  _s: 0,
  set value(v: number) {
    this._s = v * 2;
  },
  get value() {
    return this._s;
  },
};
const child: any = Object.create(protoSet);
setValue(child, 21);
log("setter", getValue(child), Object.keys(child).join(","));
// accessor on the receiver's own class prototype
class WithSetter {
  _v = 0;
  get value() {
    return this._v;
  }
  set value(v: number) {
    this._v = v + 1;
  }
}
const ws = new WithSetter();
setValue(ws, 5);
log("class setter", getValue(ws), Object.keys(ws).join(","));
// 5. non-writable inherited and own
const roProto = Object.defineProperty({}, "value", { value: 11, writable: false });
const ro: any = Object.create(roProto);
setValue(ro, 12);
log("inherited non-writable", getValue(ro), Object.keys(ro).join(","));
const ownRo: any = new Num();
Object.defineProperty(ownRo, "value", { writable: false, value: 4 });
setValue(ownRo, 8);
log("own non-writable", getValue(ownRo));
let threw = "no";
try {
  (function () {
    "use strict";
    const q: any = new Num();
    Object.defineProperty(q, "value", { writable: false, value: 4 });
    q.value = 6;
  })();
} catch (e) {
  threw = (e as Error).constructor.name;
}
log("strict non-writable", threw);
// 6. freeze / seal / preventExtensions
const fr: any = new Num();
Object.freeze(fr);
setValue(fr, 99);
log("frozen", getValue(fr));
const se: any = new Num();
Object.seal(se);
setValue(se, 98);
log("sealed", getValue(se), Object.isSealed(se));
const pe: any = new Num();
Object.preventExtensions(pe);
setValue(pe, 97);
setNext(pe, 96);
log("noextend", getValue(pe), getNext(pe));
const peLit: any = { tag: "p" };
Object.preventExtensions(peLit);
setValue(peLit, 1);
log("noextend literal", getValue(peLit), Object.keys(peLit).join(","));
// 7. holes then a store (delete then re-add), key order
const h: any = new Num();
delete h.value;
log("deleted", getValue(h), Object.keys(h).join(","));
setValue(h, 5);
log("refilled", getValue(h), Object.keys(h).join(","));
const h2: any = new Num();
delete h2.value;
delete h2.next;
setNext(h2, 1);
setValue(h2, 2);
log("order", Object.keys(h2).join(","));
const lit1: any = { next: 0, tag: "x" };
setValue(lit1, 3);
log("literal add", Object.keys(lit1).join(","), getValue(lit1));
const lit2: any = { value: 1, next: 2 };
delete lit2.value;
setValue(lit2, 7);
log("literal readd", Object.keys(lit2).join(","));
const late = new Late();
setValue(late, 2.5);
setValue(late, "w");
log("late", getValue(late), Object.keys(late).join(","));
// 8. receivers that are not plain objects
const arr: any = [1, 2];
setValue(arr, 5);
log("array", getValue(arr), Object.keys(arr).join(","));
const fn: any = function () {};
setValue(fn, 6);
log("function", getValue(fn));
for (const bad of [undefined, null]) {
  try {
    setValue(bad, 1);
    log("nullish", "no throw");
  } catch (e) {
    log("nullish", (e as Error).constructor.name);
  }
}
const prim: any = 5;
setValue(prim, 1);
log("primitive", getValue(prim));
// 9. an object used as a prototype keeps working
const pr: any = new Num();
const kid: any = Object.create(pr);
setValue(pr, 10);
log("proto holder", getValue(kid));
setValue(kid, 11);
log("kid own", getValue(kid), getValue(pr), Object.keys(kid).join(","));
console.log(out.join("\n"));
