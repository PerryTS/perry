// DEC 250 (Q2): in a compiled program the built-in iterator prototypes
// `%ArrayIteratorPrototype%`, `%MapIteratorPrototype%`, `%SetIteratorPrototype%`,
// `%StringIteratorPrototype%` and `%RegExpStringIteratorPrototype%` are locked: replacing, redefining or deleting
// their `next` is a TypeError, and every driver (for-of, spread, Array.from,
// call spread, manual calls) keeps the builtin. Perry differs from node here
// on purpose: tests/parity/expected holds Perry's output.
// Each family is tried with every way of changing `next`: a plain function, a
// bound copy of the original (same algorithm, a different function object), an
// accessor, a delete, a non-callable value, and the original closure itself.
// A user-defined `next` on an iterator OBJECT is still honoured per call
// (test_gap_iterator_patched_next), and so is a foreign prototype
// (test_gap_iterator_next_foreign_prototype); this file covers the prototypes.

const arrayProto: any = Object.getPrototypeOf([][Symbol.iterator]());
const mapProto: any = Object.getPrototypeOf(new Map().entries());
const setProto: any = Object.getPrototypeOf(new Set().values());
const stringProto: any = Object.getPrototypeOf(""[Symbol.iterator]());
const regexpProto: any = Object.getPrototypeOf("a".matchAll(/a/g));

function refused(change: () => void): string {
  try {
    change();
    return "changed";
  } catch (e: any) {
    if (e instanceof TypeError) console.log("DEC 250: " + e.message);
    return e instanceof TypeError ? "TypeError" : "other";
  }
}

function tryAll(label: string, proto: any, other: any) {
  const orig = proto.next;
  const forms = [
    refused(() => { proto.next = function () { return { done: true, value: undefined }; }; }),
    refused(() => { proto.next = orig.bind(other); }),
    refused(() => { Object.defineProperty(proto, "next", { configurable: true, get() { return orig; } }); }),
    refused(() => { delete proto.next; }),
    refused(() => { proto.next = 42; }),
    refused(() => { proto.next = orig; }),
  ];
  console.log(label + "-refused", forms.join(","), proto.next === orig);
}

// A: the array family.
tryAll("A", arrayProto, [100, 200].values());
{
  const got: number[] = [];
  for (const v of [1, 2, 3]) got.push(v);
  console.log("A-forof", got.join(","));
  console.log("A-spread", [...[4, 5]].join(","));
  console.log("A-from", Array.from([6].values()).join(","));
  console.log("A-from-array", Array.from([11, 12]).join(","));
  console.log("A-call-spread", ((...xs: number[]) => xs.join(","))(...[13, 14]));
  console.log("A-multi-spread", [...[15], ...[16]].join(","));
  const it = [7, 8].values();
  console.log("A-manual", it.next().value, it.next().value, it.next().done);
}

// D: the Map and Set families.
tryAll("D-map", mapProto, new Map([["q", 9]]).entries());
{
  const got: string[] = [];
  for (const [k, v] of new Map([["a", 1], ["b", 2]])) got.push(k + "=" + v);
  console.log("D-map", got.join(","));
  console.log("D-map-spread", JSON.stringify([...new Map([["z", 5]])]));
  console.log("D-map-from", JSON.stringify(Array.from(new Map([["y", 6]]))));
  const it = new Map([["m", 1]]).entries();
  console.log("D-map-manual", JSON.stringify(it.next().value), it.next().done);
}
tryAll("D-set", setProto, new Set([100, 200]).values());
{
  console.log("D-set", [...new Set([1, 2])].join(","));
  console.log("D-set-from", Array.from(new Set([3, 4])).join(","));
  const got: number[] = [];
  for (const v of new Set([5, 6])) got.push(v);
  console.log("D-set-forof", got.join(","));
  console.log("D-set-call-spread", ((...xs: number[]) => xs.join(","))(...new Set([7, 8])));
}

// E: the String family.
tryAll("E", stringProto, "zz"[Symbol.iterator]());
{
  console.log("E-string", [..."ab"].join(","));
  console.log("E-string-from", Array.from("cd").join(","));
  let text = "";
  for (const ch of "a😀b") text += ch + "|";
  console.log("E-string-forof", text);
  const it = "xy"[Symbol.iterator]();
  console.log("E-manual", it.next().value, it.next().value, it.next().done);
}

// R: the RegExp String family.
tryAll("R", regexpProto, "bb".matchAll(/b/g));
{
  console.log("R-spread", [..."a1a2".matchAll(/a(\d)/g)].map((m) => m[1]).join(","));
  const it = "c3".matchAll(/c(\d)/g);
  const first = it.next();
  console.log("R-manual", first.value![1], it.next().done);
}

// G: the @@toStringTag of every family is locked too.
for (const [label, proto, it] of [
  ["array", arrayProto, [1].values()],
  ["map", mapProto, new Map().entries()],
  ["set", setProto, new Set().values()],
  ["string", stringProto, ""[Symbol.iterator]()],
  ["regexp", regexpProto, "a".matchAll(/a/g)],
] as [string, any, any][]) {
  const forms = [
    refused(() => { proto[Symbol.toStringTag] = "X"; }),
    refused(() => { delete proto[Symbol.toStringTag]; }),
  ];
  console.log("G-" + label, forms.join(","), Object.prototype.toString.call(it));
}

// F: adding a NEW member to a locked prototype is allowed; it does not
// change iteration, and it can be deleted again.
{
  setProto.extra = 1;
  console.log("F-added", setProto.extra, [...new Set([1, 2])].join(","));
  delete setProto.extra;
  console.log("F-deleted", "extra" in setProto);
}
