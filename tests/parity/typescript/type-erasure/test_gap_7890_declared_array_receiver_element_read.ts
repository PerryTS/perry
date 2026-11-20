// #7890: a property read used DIRECTLY as an element-read receiver
// (`e.items[i]`, `e.items.length`) now takes the receiver's declared array type
// from the annotation, the same claim #7854 already gave `const xs = e.items`.
//
// #7854's own test (`test_gap_declared_field_type_refine_guarded.ts`) always
// routes through an intermediate local, so it does not cover this shape. Here
// the receiver is the `PropertyGet` itself — through a `type` alias, an
// `interface`, a class, a nullable union, a reassigned cursor, and a nested
// chain — and every row is handed a value that violates the declaration.
//
// A claim is admissible here only because the tier it unlocks re-checks
// `GC_TYPE_ARRAY`, forwarding, descriptors, the prototype latch and the bounds
// on the receiver itself. If any of those guards were dropped, the non-array
// rows below would print garbage (or crash) instead of the JavaScript answer.

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

// No intermediate local anywhere below — `e.items` IS the receiver.
function readAlias(head: Bag | null): string {
  let e: Bag | null = head;
  let out = "";
  while (e !== null) {
    out = out + e.label + "|len=" + e.items.length;
    for (let i = 0; i < 2; i++) {
      out = out + "|" + i + "=" + e.items[i];
    }
    e = null;
  }
  return out;
}

function readIface(head: IBag | null): string {
  let e: IBag | null = head;
  let out = "";
  while (e !== null) {
    out = out + e.label + "|len=" + e.items.length + "|0=" + e.items[0];
    e = null;
  }
  return out;
}

function readClass(head: CBag | null): string {
  let e: CBag | null = head;
  let out = "";
  while (e !== null) {
    out = out + e.label + "|len=" + e.items.length + "|0=" + e.items[0];
    e = null;
  }
  return out;
}

// Honest rows first — the ones the optimization exists for.
console.log(readAlias(mkAlias(["a", "b", "c"])));
console.log(readIface(mkIface(["x"])));
console.log(readClass(mkClass(["y", "z"])));

// Every row below violates the declared type.
console.log(readAlias(mkAlias("hello"))); // string: has .length, indexes to chars
console.log(readAlias(mkAlias({ length: 7, 0: "zero" }))); // plain object aping an array
console.log(readAlias(mkAlias({ length: "seven" }))); // non-numeric length
console.log(readAlias(mkAlias(42))); // number: no .length, no index
console.log(readIface(mkIface(new Uint8Array(3)))); // typed array: off-heap, no GcHeader

function twoArgs(a: any, b: any): void {
  void a;
  void b;
}
console.log(readClass(mkClass(twoArgs))); // function: .length is the param count

// A nullish field value must still THROW on `.length`, not read a header.
function readCaught(v: any): string {
  try {
    return readAlias(mkAlias(v));
  } catch (error) {
    const caught = error as Error;
    return "threw:" + (caught instanceof TypeError);
  }
}
console.log(readCaught(null));
console.log(readCaught(undefined));

// A non-numeric / non-integer / negative / out-of-range index on the same
// receiver shape — these leave the integer-index proof and take the runtime
// key path, which must still validate the receiver.
function oddIndexes(b: Bag): string {
  let out = "";
  out = out + "|neg=" + b.items[-1];
  out = out + "|frac=" + b.items[0.5];
  out = out + "|far=" + b.items[99];
  return out;
}
console.log(oddIndexes(mkAlias(["only"])));
console.log(oddIndexes(mkAlias({ length: 1 })));
console.log(oddIndexes(mkAlias("s")));

// A nested chain: a declared read feeding another declared read, still with no
// intermediate local.
type Outer = { inner: Bag; tag: string };
function readOuter(o: Outer | null): string {
  let e: Outer | null = o;
  let out = "";
  while (e !== null) {
    out = out + e.tag + "/" + e.inner.label + "/" + e.inner.items.length + "/" + e.inner.items[0];
    e = null;
  }
  return out;
}
console.log(readOuter({ inner: mkAlias(["deep"]), tag: "o" }));
console.log(readOuter({ inner: mkAlias(3.5), tag: "o" }));

// A store through the same receiver shape must stay on the guarded path too.
function writeThrough(b: Bag, v: string): string {
  b.items[0] = v;
  return "" + b.items[0] + "/" + b.items.length;
}
console.log(writeThrough(mkAlias(["old"]), "new"));
console.log(writeThrough(mkAlias({ length: 1 }), "new"));

// The element's DECLARED type is `string` but its runtime value is not: `===`
// must not take a string-only comparison and `+` must not take a concat-only
// lowering.
function scan(b: Bag, needle: string): string {
  let out = "";
  for (let i = 0; i < 3; i++) {
    out =
      out +
      "|" +
      (b.items[i] === needle) +
      "," +
      typeof b.items[i] +
      "," +
      (b.items[i] + "!");
  }
  return out;
}
console.log(scan(mkAlias(["a", "b"]), "a"));
console.log(scan(mkAlias([1, 2, 3] as any), "a"));
console.log(scan(mkAlias({ length: 3 }), "a"));

// A STRING-literal key on the same receiver shape is deliberately NOT admitted
// to the numeric array tier by #7890 — see `index_get.rs`. #7891 routes it
// through an SSO-tag guard plus heap-header classification so a violated
// declaration keeps enough receiver identity for string indexing.
function stringKey(b: Bag): string {
  return (
    "" + b.items["length"] + "/" + b.items["nope"] + "/" + typeof b.items["constructor"]
  );
}
console.log(stringKey(mkAlias(["k"])));
console.log(stringKey(mkAlias({ length: 2, 0: "obj0", nope: "here" })));
console.log(stringKey(mkAlias(9)));
