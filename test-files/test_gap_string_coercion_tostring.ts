// #321 (effect Redacted): object→string coercion must honor a custom
// `toString()` method (own OR inherited), per `ToPrimitive` /
// `OrdinaryToPrimitive`. `String(obj)`, template literals, and `"" + obj`
// all coerce with hint "string": try `toString` first, then `valueOf`.
//
// Pre-fix, Perry's coercion only consulted `[Symbol.toPrimitive]` and then
// fell straight back to the default `Object.prototype.toString`
// (`"[object Object]"`), so a user-defined `toString` was ignored. This
// affects ANY object with a custom `toString`, surfaced via effect's
// `Redacted` (`String(secret)` → `"<redacted>"`).
//
// Compared byte-for-byte against `node --experimental-strip-types`.

// (1) OWN custom toString — String(), template literal, and `+` all honor it.
const a: any = {
  toString() {
    return "CUSTOM-A";
  },
};
console.log("own:", String(a), `${a}`, "" + a); // CUSTOM-A CUSTOM-A CUSTOM-A

// (2) INHERITED custom toString via Object.create — resolved off the
//     prototype chain with `this` bound to the receiver.
const Proto: any = {
  toString() {
    return "CUSTOM-P";
  },
};
const b = Object.create(Proto);
console.log("proto:", String(b), `${b}`); // CUSTOM-P CUSTOM-P

// (3) Symbol.toPrimitive still takes priority over toString (already worked).
const c: any = {
  [Symbol.toPrimitive](hint: string) {
    return "PRIM-" + hint;
  },
};
console.log("toPrimitive:", String(c), `${c}`); // PRIM-string PRIM-string

// (4) toString reads `this` — own and inherited.
const t: any = {
  name: "Bob",
  toString() {
    return "Name=" + this.name;
  },
};
console.log("this-own:", String(t)); // Name=Bob
const GP: any = {
  greeting: "hi",
  toString() {
    return this.greeting + "!";
  },
};
const g = Object.create(GP);
g.greeting = "yo";
console.log("this-inherited:", String(g)); // yo!

// (5) class instance with a toString method.
class K {
  toString() {
    return "K-INST";
  }
}
console.log("class:", String(new K()), `${new K()}`, "" + new K()); // K-INST x3

// (6) toString that returns a NUMBER primitive — coerced to its string form.
const num: any = {
  toString() {
    return 42;
  },
};
console.log("num-toString:", String(num), `${num}`); // 42 42

// (7) toString returns a NON-primitive (object) → spec falls through to
//     valueOf.
const fallback: any = {
  toString() {
    return {};
  },
  valueOf() {
    return "VALUEOF";
  },
};
console.log("toString-nonprimitive:", String(fallback)); // VALUEOF

// (8) hint "string" with ONLY valueOf (no custom toString): the default
//     Object.prototype.toString wins, so `valueOf` is NOT consulted.
const VP: any = {
  valueOf() {
    return 99;
  },
};
const vo = Object.create(VP);
console.log("valueOf-only-string-hint:", String(vo), `${vo}`); // [object Object] x2

// (9) REGRESSION GUARDS — must keep matching Node exactly.
console.log("plain:", String({}), `${{}}`, "" + {}); // [object Object] x3
console.log("array:", String([1, 2, 3])); // 1,2,3
class L {
  x = 5;
}
console.log("class-no-tostring:", String(new L())); // [object Object]
console.log(
  "primitives:",
  String(42),
  String(null),
  String(undefined),
  String(true),
  String(false),
  String(3.14),
); // 42 null undefined true false 3.14
