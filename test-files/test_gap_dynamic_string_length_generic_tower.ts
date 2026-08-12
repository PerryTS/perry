// `.length` read through a receiver the front end cannot type — the generic
// property-get tower, not the proven-string lowering.
//
// The tower now serves a NaN-boxed string (SSO and heap) inline instead of
// missing the object inline-cache and walking the runtime's object ladder.
// That short-circuit is only sound if EVERY other receiver still takes the
// tower unchanged, so this exercises the same call site with a string, an
// array, array-like objects with numeric and non-numeric `length`, a function,
// a typed array, a plain number, an object with no `length` at all, and both
// nullish values (which must throw a catchable TypeError).

type Box = { payload: any; label: string };

function boxed(value: any): Box {
  return { payload: value, label: "box" };
}

// `box.payload` is `any`, so `.length` here lowers through the generic tower.
function readLength(value: any): any {
  const box = boxed(value);
  return box.payload.length;
}

function show(label: string, value: any): void {
  const length = readLength(value);
  console.log(label, String(length), typeof length, length === undefined);
}

// SSO string (short) and heap string (long, and a concat result).
show("sso", "abc");
show("empty", "");
show("heap", "0123456789012345678901234567890123456789");
show("concat", "t:" + "alpha");
show("non-ascii", "héllo\u{1F600}");

show("array", ["a", "b"]);
show("array-like number", { length: 7, 0: "z" });
show("array-like string", { length: "seven" });
show("no length", { other: 1 });
show("number", 42);
show("boolean", true);
show("typed array", new Uint8Array(3));

function twoArgs(a: any, b: any): void {
  void a;
  void b;
}
show("function", twoArgs);

for (const value of [null, undefined]) {
  try {
    readLength(value);
    console.log("nullish", String(value), "no throw");
  } catch (error) {
    const caught = error as Error;
    console.log(
      "nullish",
      String(value),
      caught.constructor.name + ": " + caught.message,
    );
  }
}

// The same site, hot and monomorphic on strings — this is the `pipeline.ts`
// shape (`rec.tag.length` where `rec` is an object-literal type).
let total = 0;
for (let i = 0; i < 200; i++) {
  total = total + readLength("t:" + (i % 3 === 0 ? "alpha" : "be"));
}
console.log("total", String(total));
