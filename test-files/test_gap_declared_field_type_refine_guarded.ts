// #7854: `const xs = obj.field` now refines `xs` from the receiver's DECLARED
// annotation (class / interface / `type X = {…}` alias), seeing through a
// `T | null` union and a reassigned receiver. That is a CLAIM, not a proof —
// Perry does not enforce declared types at runtime — so every consumer the
// refinement newly reaches must be a guarded tier.
//
// This file hands the claim values that violate it and requires the ordinary
// JavaScript answer anyway. If the refined `string[]` ever authorised an
// unguarded element load or an unguarded ArrayHeader length read, the non-array
// rows below would print garbage (or crash) instead of `undefined`.

type Bag = { items: string[]; label: string };

interface IBag {
  items: string[];
  label: string;
}

class CBag {
  items: string[];
  label: string;
  constructor(items: string[], label: string) {
    this.items = items;
    this.label = label;
  }
}

// The lie: `v` is whatever the caller passed, stored into a slot the type
// system says is `string[]`.
function mkAlias(v: any): Bag {
  return { items: v, label: "alias" };
}
function mkIface(v: any): IBag {
  return { items: v, label: "iface" };
}
function mkClass(v: any): CBag {
  return new CBag(v, "class");
}

// Shaped like `interp.ts`'s `lookup`: a nullable, REASSIGNED cursor, so the
// refinement has to see through both `Bag | null` and `reassigned_locals`.
function readAlias(head: Bag | null): string {
  let e: Bag | null = head;
  let out = "";
  while (e !== null) {
    const items = e.items;
    out = out + e.label + "|len=" + items.length;
    for (let i = 0; i < 2; i++) {
      out = out + "|" + i + "=" + items[i];
    }
    e = null;
  }
  return out;
}

function readIface(head: IBag | null): string {
  let e: IBag | null = head;
  let out = "";
  while (e !== null) {
    const items = e.items;
    out = out + e.label + "|len=" + items.length + "|0=" + items[0];
    e = null;
  }
  return out;
}

function readClass(head: CBag | null): string {
  let e: CBag | null = head;
  let out = "";
  while (e !== null) {
    const items = e.items;
    out = out + e.label + "|len=" + items.length + "|0=" + items[0];
    e = null;
  }
  return out;
}

// Honest row first — this is the one the optimization exists for.
console.log(readAlias(mkAlias(["a", "b", "c"])));
console.log(readIface(mkIface(["x"])));
console.log(readClass(mkClass(["y", "z"])));

// Every row below violates the declared type.
console.log(readAlias(mkAlias("hello"))); // string: has .length, indexes to chars
console.log(readAlias(mkAlias({ length: 7, 0: "zero" }))); // plain object aping an array
console.log(readAlias(mkAlias(42))); // number: no .length, no index

// A nullish field value must still THROW on `.length`, not read a header.
function readCaught(v: any): string {
  try {
    return readAlias(mkAlias(v));
  } catch (err) {
    return "threw:" + (err instanceof TypeError);
  }
}
console.log(readCaught(null));
console.log(readCaught(undefined));

console.log(readIface(mkIface({ length: 3 })));
console.log(readClass(mkClass("qq")));

// A nested read chain: the refined local feeds another declared-type read.
type Outer = { inner: Bag; tag: string };
function readOuter(o: Outer | null): string {
  let e: Outer | null = o;
  let out = "";
  while (e !== null) {
    const inner = e.inner;
    const items = inner.items;
    out = out + e.tag + "/" + inner.label + "/" + items.length + "/" + items[0];
    e = null;
  }
  return out;
}
console.log(readOuter({ inner: mkAlias(["deep"]), tag: "o" }));
console.log(readOuter({ inner: mkAlias(3.5), tag: "o" }));

// Mutation through the refined local must still go through the normal
// (guarded) array store path when the receiver really is an array, and must
// stay a plain property write when it is not.
function pushish(b: Bag, v: string): string {
  const items = b.items;
  items[0] = v;
  return "" + items[0] + "/" + items.length;
}
console.log(pushish(mkAlias(["old"]), "new"));
console.log(pushish(mkAlias({ length: 1 }), "new"));

// An element read whose DECLARED element type is `string` but whose runtime
// value is not: `===` against a string literal must not take a string-only
// comparison, and `+` must not take a concat-only lowering.
function scan(b: Bag, needle: string): string {
  const items = b.items;
  let out = "";
  for (let i = 0; i < 3; i++) {
    const v = items[i];
    out = out + "|" + (v === needle) + "," + (v === 7) + "," + typeof v + "," + (v + "!");
  }
  return out;
}
console.log(scan(mkAlias(["a", "7", "c"]), "a"));
console.log(scan(mkAlias([1, 7, true]), "a"));
console.log(scan(mkAlias([null, undefined, { z: 1 }]), "a"));
