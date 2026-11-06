function assertEqual(label: string, actual: any, expected: any) {
  if (actual !== expected) {
    throw new Error(label + ": expected " + expected + ", got " + actual);
  }
}

function assertTrue(label: string, actual: any) {
  if (!actual) {
    throw new Error(label + ": expected truthy, got " + actual);
  }
}

function strictThisKind(this: any) {
  "use strict";
  return typeof this;
}

function sloppyThisKind(this: any) {
  return typeof this;
}

function strictThisValue(this: any) {
  "use strict";
  return this;
}

assertEqual("strict call number this", strictThisKind.call(1), "number");
assertEqual("sloppy call number boxes this", sloppyThisKind.call(1), "object");
assertEqual("sloppy direct call gets global this", sloppyThisKind(), "object");

const callValue = Function.prototype.call;
assertEqual("first-class Function.prototype.call", callValue.call(strictThisKind, 1), "number");

const applyValue = Function.prototype.apply;
assertEqual("first-class Function.prototype.apply", applyValue.call(strictThisKind, 1, []), "number");

function strictReplace(this: any) {
  "use strict";
  return this === undefined ? "U" : "T";
}

function sloppyReplace(this: any) {
  return typeof this;
}

assertEqual("string replace strict function receiver", "ab".replace("b", strictReplace), "aU");
assertEqual(
  "string replace sloppy function receiver",
  "ab".replace("b", sloppyReplace),
  "aobject",
);

const n: any = 5;
n.issue3586Temp = 7;
assertEqual("primitive temp property write", typeof n.issue3586Temp, "undefined");

Object.defineProperty(Object.prototype, "__issue3586_sloppy_getter", {
  get: function () {
    return this;
  },
  configurable: true,
});

Object.defineProperty(Object.prototype, "__issue3586_strict_getter", {
  get: function () {
    "use strict";
    return this;
  },
  configurable: true,
});

const sloppyGetterValue = (5 as any).__issue3586_sloppy_getter;
assertEqual("sloppy primitive getter boxes this", typeof sloppyGetterValue, "object");
assertTrue("sloppy primitive getter loose equality", sloppyGetterValue == 5);
assertEqual("strict primitive getter keeps primitive", (5 as any).__issue3586_strict_getter, 5);
assertEqual("strict primitive getter typeof", typeof (5 as any).__issue3586_strict_getter, "number");

function functionConstructorFromStrictContext() {
  "use strict";
  return Function("return typeof this")();
}

assertEqual("Function constructor body remains sloppy", functionConstructorFromStrictContext(), "object");

console.log("issue-3586-ok");
