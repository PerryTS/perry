// Object model case 2c: a class family's layout holds the keys static-name
// stores add after construction (late keys) as slots that are holes until
// written. Every observation must see only the keys written so far, in the
// order they were written, and an unwritten key must resolve through the
// prototype chain.
import * as util from "node:util";

function show(label: string, o: any) {
  const forIn: string[] = [];
  for (const k in o) forIn.push(k);
  console.log(
    label,
    Object.keys(o).join(","),
    "|", forIn.join(","),
    "|", JSON.stringify(o),
    "|", JSON.stringify({ ...o }),
    "|", JSON.stringify(Object.assign({}, o)),
    "|", Object.getOwnPropertyNames(o).join(","),
    "|", Reflect.ownKeys(o).map(String).join(","),
    "|", JSON.stringify(Object.entries(o)),
    "|", Object.values(o).join(","),
  );
  console.log(
    label,
    "x:", o.x, "x" in o, Object.hasOwn(o, "x"), o.hasOwnProperty("x"),
    o.propertyIsEnumerable("x"), JSON.stringify(Object.getOwnPropertyDescriptor(o, "x")),
    "y:", o.y, "y" in o, Object.hasOwn(o, "y"),
    "z:", o.z, "z" in o, Object.hasOwn(o, "z"),
  );
  console.log(label, util.inspect(o), JSON.stringify(Object.getOwnPropertyDescriptors(o)));
}

class Fam {
  constructor(a: any, b: any) {
    (this as any).a = a;
    (this as any).b = b;
  }
  touch() {
    return (this as any).a;
  }
}
class FamSub extends Fam {
  constructor(a: any, b: any) {
    super(a, b);
  }
}

// Static-name stores through receivers that read the family's own keys:
// x, y and z are late keys of Fam (and of FamSub, which inherits the slots).
function setX(o: any, v: any) { if (o.a !== 0 && o.b !== 0) o.x = v; }
function setY(o: any, v: any) { if (o.b !== 0 && o.a !== 0) o.y = v; }
function setZ(o: any, v: any) { o.a; o.b; o.z = v; }

const p: any = new Fam(1, 2);
show("fresh", p);
setX(p, 10);
show("x", p);
setY(p, undefined);
show("xy", p);
setZ(p, 30);
show("xyz", p);

// Written out of the layout's order: enumeration follows the writes.
const q: any = new Fam(3, 4);
setZ(q, 31);
setX(q, 11);
show("zx", q);
setY(q, 21);
show("zxy", q);

// An unwritten late key reads through the prototype chain.
(Fam.prototype as any).y = "from-proto";
const r: any = new FamSub(5, 6);
show("proto", r);
setY(r, "own");
show("proto-own", r);
delete r.y;
show("proto-deleted", r);
delete (Fam.prototype as any).y;
show("proto-gone", r);

// Delete, then re-add, of a late key and of a constructor key.
const s: any = new Fam(7, 8);
setX(s, 12);
setY(s, 22);
delete s.x;
show("del-x", s);
setX(s, 13);
show("readd-x", s);
delete s.a;
show("del-a", s);
s.a = 9;
show("readd-a", s);

// defineProperty and integrity on an object with unwritten late keys.
const t: any = new Fam(1, 1);
Object.defineProperty(t, "y", { value: 5, enumerable: false, writable: false, configurable: true });
show("define-y", t);
try { setY(t, 6); } catch (e: any) { console.log("throws", e.constructor.name); }
show("define-y-write", t);
const u: any = new Fam(2, 2);
setX(u, 1);
Object.freeze(u);
try { setY(u, 2); } catch (e: any) { console.log("throws", e.constructor.name); }
console.log("frozen", Object.isFrozen(u), Object.isSealed(u), Object.isExtensible(u), "y" in u, Object.keys(u).join(","));
const w: any = new Fam(3, 3);
Object.preventExtensions(w);
try { setX(w, 1); } catch (e: any) { console.log("throws", e.constructor.name); }
console.log("nonext", Object.isExtensible(w), "x" in w, Object.keys(w).join(","), Object.isFrozen(w), Object.isSealed(w));
const w2: any = new Fam(4, 4);
Object.seal(w2);
console.log("sealed-fresh", Object.isFrozen(w2), Object.isSealed(w2), Object.keys(w2).join(","));

// structuredClone and spread into a literal.
const v: any = new Fam(4, 5);
setY(v, 50);
console.log("clone", JSON.stringify(structuredClone(v)), JSON.stringify({ k: 1, ...v }));

// A computed write of a late key meets its slot; a computed read sees it.
const c: any = new Fam(6, 7);
const key = ["z", "x"][Number(process.argv.length > 99)];
(c as any)[key] = 70;
console.log("computed", (c as any)[key], c.z, Object.keys(c).join(","));
setZ(c, 71);
console.log("computed-static", (c as any)[key], Object.keys(c).join(","));

// A setter, then a read-only key, defined on the prototype after births: an
// unwritten late key's store must meet them.
const st: any = new Fam(8, 9);
const st2: any = new Fam(8, 9);
setX(st2, 1);
Object.defineProperty(Fam.prototype, "z", {
  set(v: any) { console.log("setter z", v, this === st); },
  get() { return "getter z"; },
  configurable: true,
});
setZ(st, 5);
show("setter", st);
setZ(st2, 6);
show("setter2", st2);
delete (Fam.prototype as any).z;
Object.defineProperty(Fam.prototype, "y", { value: "ro", writable: false, configurable: true });
try { setY(st, 7); } catch (e: any) { console.log("throws", e.constructor.name); }
(function () { "use strict"; try { setY(st, 7); } catch (e: any) { console.log("strict throws", e.constructor.name); } })();
show("readonly", st);
delete (Fam.prototype as any).y;
setY(st, 8);
show("after-readonly", st);

// Every writer of an unwritten late key: compound, logical, update,
// destructuring, Reflect.set, Object.assign, computed, defineProperty.
const m: any = new Fam(1, 2);
m.a; m.b;
m.z += 1;
show("compound", m);
const m2: any = new Fam(1, 2);
m2.y ??= "q";
m2.x ||= "r";
show("logical", m2);
const m3: any = new Fam(1, 2);
m3.x++;
show("update", m3);
const m4: any = new Fam(1, 2);
({ p: m4.z, q: m4.x } = { p: 1, q: 2 });
show("destructure", m4);
const m5: any = new Fam(1, 2);
Reflect.set(m5, "y", 3);
Reflect.set(m5, "x", 4);
show("reflect", m5);
const m6: any = new Fam(1, 2);
Object.assign(m6, { z: 1 }, { x: 2 });
show("assign", m6);
const m7: any = new Fam(1, 2);
Object.defineProperty(m7, "z", { value: 1, writable: true, enumerable: true, configurable: true });
setX(m7, 2);
show("define-z", m7);
const m8: any = new Fam(1, 2);
Object.defineProperties(m8, { y: { value: 1, enumerable: true }, x: { get() { return "gx"; }, enumerable: true, configurable: true } });
show("define-props", m8);

// An object used as a prototype, then given a late key; a fresh object
// inheriting from it sees it.
const base: any = new Fam(1, 2);
const kid: any = Object.create(base);
console.log("kid-before", kid.x, "x" in kid);
setX(base, "bx");
console.log("kid-after", kid.x, "x" in kid, Object.keys(kid).join(","));

// Many objects, in and out of order, then sort and stringify.
const many: any[] = [];
for (let i = 0; i < 50; i++) {
  const o: any = new Fam(i, i + 1);
  if (i % 3 === 0) { setX(o, i); setY(o, i); setZ(o, i); }
  else if (i % 3 === 1) { setZ(o, i); setY(o, i); }
  else { setY(o, i); }
  many.push(o);
}
console.log("many", many.map((o) => Object.keys(o).join("")).join(" "));
console.log("many-json", JSON.stringify(many.slice(0, 6)));
