// #9463 — two `util.format` policy gaps, byte-compared against
// `node --experimental-strip-types`.
//
//   1. `%s` on an object applied `String(value)` unconditionally. Node applies
//      it only when the value has a USER-defined `toString`; an object whose
//      `toString` is a built-in one is INSPECTED instead, with `{ depth: 0 }`.
//      So `%s` on `[1, , 3]` is `[ 1, <1 empty item>, 3 ]`, not `1,,3`, and a
//      nested object collapses to `[Object]` rather than being walked.
//   2. `%o` is `util.inspect(value, { showHidden: true, depth: 4 })`. An
//      array's own `length` is a non-enumerable property, so node prints
//      `[length]: N` after the elements of every array, at every depth — even
//      for `[]`, where `%o` is `[ [length]: 0 ]` while `%O` is `[]`.
//
// The `%O` rows are controls: `%O` is the default-options inspect and none of
// them may move. So are the `%s`-on-a-primitive rows, and the two objects that
// DO define their own `toString` — those must keep using it.
//
// Deliberately NOT covered, each a separate pre-existing divergence measured
// while building this and reported alongside it:
//   - `%s` on a BigInt: node's `formatBigInt` keeps the `n` suffix (`5n`),
//     perry prints `5`.
//   - `%s` on a Date (node inspects to the ISO form) or an Error (node prints
//     the stack): neither is an ordinary object in perry's model.
//   - `%s` / `String()` on a class: `#9468` / `value/to_string.rs:1042`.
//   - `%o` on an object nested four deep, where node's `compact: 3` rule breaks
//     the line and perry keeps it on one; and on arrays long enough for node's
//     `groupArrayElements` column layout.

import util from "node:util";

class WithToString {
  toString(): string {
    return "CUSTOM-TO-STRING";
  }
}
class PlainInstance {
  v = 1;
}

const show = (label: string, value: string): void =>
  console.log(label, JSON.stringify(value));

// --- 1. %s on objects: inspect, not String() ---
show("s-array", util.format("%s", [1, 2, 3]));
show("s-array-holes", util.format("%s", [1, , 3]));
show("s-array-sized", util.format("%s", new Array(3)));
show("s-array-nested", util.format("%s", [[1, 2], [3]]));
show("s-object", util.format("%s", { a: 1, b: "x" }));
show("s-object-nested", util.format("%s", { a: { b: { c: 1 } } }));
show("s-object-empty", util.format("%s", {}));
show("s-class-instance", util.format("%s", new PlainInstance()));
show("s-map", util.format("%s", new Map<string, number>([["a", 1]])));
show("s-set", util.format("%s", new Set([1, 2])));

// The control: a user `toString` still wins, own or inherited from a class.
show("s-literal-tostring", util.format("%s", { toString: () => "LITERAL" }));
show("s-method-tostring", util.format("%s", { toString() { return "METHOD"; } }));
show("s-class-tostring", util.format("%s", new WithToString()));

// The other control: `%s` on a primitive is untouched.
show("s-null", util.format("%s", null));
show("s-undefined", util.format("%s", undefined));
show("s-number", util.format("%s", 42));
show("s-float", util.format("%s", 1.5));
show("s-string", util.format("%s", "hi"));
show("s-boolean", util.format("%s", true));
show("s-symbol", util.format("%s", Symbol("q")));
show("s-regexp", util.format("%s", /ab/g));
show("s-function", util.format("%s", function foo() {}));
show("s-two-args", util.format("%s and %s", [1, 2], "tail"));

// console.log routes its format string through the same helper.
console.log("%s <- console-s", [1, , 3]);
console.log("%s <- console-s-obj", { a: 1 });

// --- 2. %o carries the showHidden surface ---
show("o-array", util.format("%o", [1, 2, 3]));
show("o-array-holes", util.format("%o", [1, , 3]));
show("o-array-empty", util.format("%o", []));
show("o-array-sized", util.format("%o", new Array(3)));
show("o-array-nested", util.format("%o", [[1, 2], [3]]));
show("o-array-in-object", util.format("%o", { a: [1, 2] }));
show("o-array-strings", util.format("%o", ["a", "b"]));
show("o-object", util.format("%o", { a: 1 }));
show("o-object-hidden", util.format(
  "%o",
  Object.defineProperty({ a: 1 }, "hidden", { value: 9, enumerable: false }),
));
show("o-object-getter", util.format("%o", { get g() { return 1; }, n: 2 }));
show("o-class-instance", util.format("%o", new PlainInstance()));
show("o-map", util.format("%o", new Map<string, number>([["a", 1]])));
show("o-set", util.format("%o", new Set([1, 2])));
show("o-string", util.format("%o", "hi"));
show("o-number", util.format("%o", 7));
show("o-null", util.format("%o", null));
show("o-undefined", util.format("%o", undefined));
console.log("%o <- console-o", [1, 2, 3]);

// --- 3. %O controls: none of these may move ---
show("O-array", util.format("%O", [1, 2, 3]));
show("O-array-holes", util.format("%O", [1, , 3]));
show("O-array-empty", util.format("%O", []));
show("O-array-sized", util.format("%O", new Array(3)));
show("O-array-nested", util.format("%O", [[1, 2], [3]]));
show("O-array-in-object", util.format("%O", { a: [1, 2] }));
show("O-object", util.format("%O", { a: 1 }));
show("O-object-hidden", util.format(
  "%O",
  Object.defineProperty({ a: 1 }, "hidden", { value: 9, enumerable: false }),
));
show("O-object-getter", util.format("%O", { get g() { return 1; }, n: 2 }));
show("O-class-instance", util.format("%O", new PlainInstance()));
show("O-map", util.format("%O", new Map<string, number>([["a", 1]])));
show("O-set", util.format("%O", new Set([1, 2])));
console.log("%O <- console-O", [1, 2, 3]);

// --- 4. util.inspect controls: showHidden reaches it the same way ---
show("inspect-default", util.inspect([1, 2, 3]));
show("inspect-hidden", util.inspect([1, 2, 3], { showHidden: true }));
show("inspect-hidden-empty", util.inspect([], { showHidden: true }));
show("inspect-hidden-object", util.inspect({ a: 1 }, { showHidden: true }));
