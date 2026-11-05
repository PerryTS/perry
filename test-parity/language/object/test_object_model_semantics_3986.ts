// Object-model semantics regression (#3986/#3577/#3576/#3575): Object(...)
// boxing, primitive-receiver boxing (sloppy vs strict), this-binding,
// var-hoisting / global-object alignment, and the consequences for
// declared-type widening + identity comparison.
//
// Validated byte-for-byte against `node` v26. Every assertion below matches
// Node's output.

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

// ---- Object(...) boxing ---------------------------------------------------
assertEqual("Object(true).valueOf", Object(true).valueOf(), true);
assertEqual("Object(0).valueOf", Object(0).valueOf(), 0);
assertEqual("Object('s').valueOf", Object("s").valueOf(), "s");
assertTrue("Object(true) instanceof Boolean", Object(true) instanceof Boolean);
assertTrue("Object(0) instanceof Number", Object(0) instanceof Number);
assertTrue("Object('x') instanceof String", Object("x") instanceof String);
assertEqual("typeof Object(5)", typeof Object(5), "object");
const sharedObj = { a: 1 };
assertTrue("Object(obj) identity", Object(sharedObj) === sharedObj);
const sharedArr = [1, 2];
assertTrue("Object(arr) identity", Object(sharedArr) === sharedArr);
assertTrue("Object() is plain", Object.getPrototypeOf(Object()) === Object.prototype);
assertTrue("Object(null) is plain", Object.getPrototypeOf(Object(null)) === Object.prototype);
assertTrue("new Object(5) boxes", new Object(5) instanceof Number);
assertEqual("Object(5) == 5", Object(5) == 5, true);
assertEqual("Object(5) === 5", Object(5) === 5, false);

// ---- primitive receiver boxing: sloppy boxes, strict sees primitive -------
(Number.prototype as any).omKind = function (this: any) {
  return typeof this;
};
(Number.prototype as any).omKindStrict = function (this: any) {
  "use strict";
  return typeof this;
};
assertEqual("sloppy number receiver boxed", (5 as any).omKind(), "object");
assertEqual("strict number receiver primitive", (5 as any).omKindStrict(), "number");
(String.prototype as any).omKind = function (this: any) {
  return typeof this;
};
assertEqual("sloppy string receiver boxed", ("a" as any).omKind(), "object");

// temporary wrapper: property writes don't persist on the primitive.
const prim: any = 5;
prim.scratch = 99;
assertEqual("primitive temp write discarded", typeof (5 as any).scratch, "undefined");
assertEqual("Object.prototype.valueOf on primitive", typeof Object.prototype.valueOf.call(true), "object");

// accessor inherited from Number.prototype sees the boxed receiver (sloppy).
Object.defineProperty(Number.prototype, "omWhoami", {
  get: function (this: any) {
    return typeof this;
  },
  configurable: true,
});
assertEqual("sloppy getter receiver boxed", (3 as any).omWhoami, "object");

// ---- this-binding ---------------------------------------------------------
function sloppyThis(this: any) {
  return this === globalThis;
}
assertTrue("sloppy direct call -> globalThis", sloppyThis());
function strictThis(this: any) {
  "use strict";
  return this;
}
assertTrue("strict direct call -> undefined", strictThis() === undefined);
function typeofThis(this: any) {
  return typeof this;
}
assertEqual("sloppy .call(1) boxes", typeofThis.call(1), "object");
assertEqual("sloppy .call(null) -> global obj", typeofThis.call(null), "object");
function typeofThisStrict(this: any) {
  "use strict";
  return typeof this;
}
assertEqual("strict .call(1) primitive", typeofThisStrict.call(1), "number");
assertTrue("strict .call(null) preserves null", (function (this: any) {
  "use strict";
  return this;
}).call(null) === null);

// a receiverless call inside a method must NOT leak the method receiver.
const recvProbe = {
  m: function (this: any) {
    function inner(this: any) {
      return this === globalThis;
    }
    return inner();
  },
};
assertTrue("nested receiverless call -> global this", recvProbe.m());

// ---- var hoisting ---------------------------------------------------------
assertEqual("var hoisted to undefined", typeof omHoisted, "undefined");
var omHoisted = 1;
assertEqual("var assigned after hoist", omHoisted, 1);

// ---- declared-type widening + identity ------------------------------------
// `widenTarget` is inferred Number from its initializer but later holds an
// object; identity comparison must still work (no NaN-boxed-pointer float
// compare).
var widenTarget: any = 2;
const widenObj = { tag: "w" };
const widenWriter = function () {
  widenTarget = widenObj;
};
widenWriter();
assertTrue("widened var identity holds", widenTarget === widenObj);
assertEqual("widened var typeof", typeof widenTarget, "object");

// ---- setter `this` from global scope (#3576 10.4.3-1-56gs) ----------------
var setterThis: any = 2;
const setterHolder = {
  set s(this: any, _v: any) {
    setterThis = this;
  },
};
setterHolder.s = 3;
assertTrue("setter this is holder", setterThis === setterHolder);

console.log("object-model-semantics: ok");
