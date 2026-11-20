"use strict";

function thrownMatches(label, expected, fn) {
  try {
    fn();
    return "none";
  } catch (e) {
    if (!e) {
      return "missing";
    }
    const ctor = e.constructor;
    if (ctor === expected) {
      return label;
    }
    if (!ctor) {
      return "undefined";
    }
    const name = ctor.name;
    return name === undefined ? "wrong:undefined" : "wrong:" + name;
  }
}

function Test262Error() {
  this.message = "";
}

console.log(
  "custom throw constructor:",
  thrownMatches("Test262Error", Test262Error, function () {
    throw new Test262Error();
  }),
);

console.log(
  "strict unresolvable:",
  thrownMatches("ReferenceError", ReferenceError, function () {
    b = 11;
  }),
);

console.log(
  "abrupt ordering:",
  thrownMatches("Test262Error", Test262Error, function () {
    throw new Test262Error();
    b = 11;
  }),
);

console.log(
  "readonly data property:",
  thrownMatches("TypeError", TypeError, function () {
    const obj = {};
    Object.defineProperty(obj, "b", { value: 1, writable: false });
    obj.b = 11;
  }),
);

console.log(
  "getter-only property:",
  thrownMatches("TypeError", TypeError, function () {
    const obj = {};
    Object.defineProperty(obj, "b", {
      get: function () {
        return 1;
      },
    });
    obj.b = 11;
  }),
);

console.log(
  "non-extensible add:",
  thrownMatches("TypeError", TypeError, function () {
    const obj = {};
    Object.preventExtensions(obj);
    obj.b = 11;
  }),
);
