// A class-typed parameter whose chain is all `number` is proved NOMINALLY:
// exact class identity plus the per-object typed-layout-intact bit, with no
// field-by-name walk. This fixture pins the receivers that must NOT take that
// fast route, by construction — each one is built so the nominal check fails
// for a different reason, and each must still produce Node-identical output
// through the generic body.

class Vec3 {
  x: number;
  y: number;
  z: number;
  constructor(x: number, y: number, z: number) {
    this.x = x;
    this.y = y;
    this.z = z;
  }
}

// All-number chain -> nominal. The loop keeps the clone from being dropped as
// consuming no proof (a thin `p.x + p.y` body lowers identically either way).
function sumVec3(p: Vec3): number {
  let s = 0;
  for (let i = 0; i < 3; i++) s += p.x * p.y + p.z + i;
  return s;
}

// One string field -> the whole chain stays on the by-name walk.
class Tagged {
  n: number;
  tag: string;
  constructor(n: number, tag: string) {
    this.n = n;
    this.tag = tag;
  }
}
function describeTagged(t: Tagged): number {
  let s = 0;
  for (let i = 0; i < 3; i++) s += t.n + t.tag.length + i;
  return s;
}

// Inherited numeric chain -> still nominal.
class Vec4 extends Vec3 {
  w: number;
  constructor(x: number, y: number, z: number, w: number) {
    super(x, y, z);
    this.w = w;
  }
}

// A numeric leaf under a string parent -> NOT nominal.
class Named {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}
class NamedCount extends Named {
  count: number;
  constructor(name: string, count: number) {
    super(name);
    this.count = count;
  }
}
function countOf(n: NamedCount): number {
  let s = 0;
  for (let i = 0; i < 3; i++) s += n.count + n.name.length + i;
  return s;
}

// Call through a dynamic route so every call reaches the public guarded entry
// rather than being resolved statically.
const api: any = { sumVec3, describeTagged, countOf };
function call(name: string, arg: any): number {
  return api[name](arg);
}

const out: string[] = [];
function show(label: string, value: number): void {
  out.push(label + "=" + String(value));
}

// --- accepted: a real instance -------------------------------------------
show("plain", call("sumVec3", new Vec3(2, 3, 4)));
show("negzero", call("sumVec3", new Vec3(-0, 3, 4)));
show("frac", call("sumVec3", new Vec3(0.5, 0.25, 1.5)));
show("nan", call("sumVec3", new Vec3(NaN, 1, 1)));
show("inf", call("sumVec3", new Vec3(Infinity, 1, 1)));
show("big", call("sumVec3", new Vec3(1e308, 10, 1)));

// --- rejected: a SUBCLASS instance ---------------------------------------
// Extra fields the proof never named; must still compute the base's view.
show("subclass", call("sumVec3", new Vec4(2, 3, 4, 5)));
const v4 = new Vec4(1, 2, 3, 4);
show("subclass_own", call("sumVec3", v4) + v4.w);

// --- rejected: Object.create(C.prototype) --------------------------------
// Right prototype, never ran the constructor, so no class-allocated layout.
const created: any = Object.create(Vec3.prototype);
created.x = 2;
created.y = 3;
created.z = 4;
show("object_create", call("sumVec3", created));
show("object_create_proto", (Object.getPrototypeOf(created) === Vec3.prototype) ? 1 : 0);
const createdEmpty: any = Object.create(Vec3.prototype);
show("object_create_empty", call("sumVec3", createdEmpty));

// --- rejected: a SHAPE-BROKEN instance -----------------------------------
// A real instance whose numeric slot is overwritten with a string retires the
// typed layout. JS still defines the arithmetic, so the answer must match.
const broken: any = new Vec3(2, 3, 4);
broken.x = "5";
show("broken_string", call("sumVec3", broken));
const broken2: any = new Vec3(2, 3, 4);
broken2.y = null;
show("broken_null", call("sumVec3", broken2));
const broken3: any = new Vec3(2, 3, 4);
broken3.z = undefined;
show("broken_undef", call("sumVec3", broken3));
const broken4: any = new Vec3(2, 3, 4);
broken4.extra = 9;
show("broken_added_field", call("sumVec3", broken4));
const broken5: any = new Vec3(2, 3, 4);
delete broken5.y;
show("broken_deleted", call("sumVec3", broken5));

// --- rejected: a same-shaped PLAIN OBJECT --------------------------------
// Identical keys and values, no class id at all.
show("plain_object", call("sumVec3", { x: 2, y: 3, z: 4 }));
show("plain_object_extra", call("sumVec3", { x: 2, y: 3, z: 4, w: 5 }));
show("plain_object_order", call("sumVec3", { z: 4, y: 3, x: 2 }));

// --- rejected: frozen / accessor receivers -------------------------------
const frozen: any = new Vec3(2, 3, 4);
Object.freeze(frozen);
show("frozen", call("sumVec3", frozen));
const withGetter: any = new Vec3(2, 3, 4);
Object.defineProperty(withGetter, "x", { get: () => 7, configurable: true });
show("accessor", call("sumVec3", withGetter));

// --- the non-nominal (by-name walk) path still works ---------------------
show("tagged", call("describeTagged", new Tagged(5, "abc")));
show("tagged_broken", (() => { const t: any = new Tagged(5, "abc"); t.tag = 12345; return call("describeTagged", t); })());
show("named_count", call("countOf", new NamedCount("ab", 7)));

// --- a receiver reused after breaking, to catch a sticky verdict ---------
const reused: any = new Vec3(2, 3, 4);
show("reused_before", call("sumVec3", reused));
reused.x = "5";
show("reused_after", call("sumVec3", reused));
reused.x = 2;
show("reused_restored", call("sumVec3", reused));

console.log(out.join("\n"));
