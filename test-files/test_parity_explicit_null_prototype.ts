// #10827: an explicitly ended `[[Prototype]]` chain must end the READ too.
//
// Perry bakes class ids at allocation time, so every "the own-key scan missed,
// what does this object inherit?" path in the runtime falls back to the
// receiver's CLASS surface -- its vtable, its declaration prototype, or
// `Object.prototype`. That fallback is right for an object whose chain was
// never touched. For one whose chain was explicitly ended it walked up anyway
// and answered from the prototype the object was BORN with.
//
// Every row prints the READ and the `in` result together, because the bug's
// signature is that perry CONTRADICTED ITSELF: `o.a` answered `1` while
// `"a" in o` answered `false`, on the same object, in the same statement.
// A fixture that printed only the read would be repairable by making `in`
// wrong too; this one cannot be.

function show(tag: string, o: any, k: string): void {
  let read: any;
  try {
    read = o[k];
  } catch (e: any) {
    read = "THROW:" + e.message;
  }
  let has: any;
  try {
    has = k in o;
  } catch (e: any) {
    has = "THROW";
  }
  let proto: any;
  try {
    proto = Object.getPrototypeOf(o);
  } catch (e: any) {
    proto = "THROW";
  }
  const kind = read === undefined ? "undefined" : typeof read;
  console.log(tag, "read=" + kind, "in=" + String(has), "proto=" + (proto === null ? "null" : typeof proto));
}

const P: any = { a: 1, m: function () { return 1; } };
function C(this: any) {}
(C as any).prototype.a = 2;
(C as any).prototype.m = function () { return 2; };
class K { m() { return 3; } }
(K as any).prototype.a = 3;

// 1 -- Object.create(P) receiver, chain ended. Data AND method: the method
// arm goes through a different resolution path than the data arm.
const o1: any = Object.create(P);
Object.setPrototypeOf(o1, null);
show("1a create+null data  ", o1, "a");
show("1b create+null method", o1, "m");

// 2 -- `new C()` receiver (synthetic class id, class-default prototype link).
const o2: any = new (C as any)();
Object.setPrototypeOf(o2, null);
show("2a newC+null data    ", o2, "a");
show("2b newC+null method  ", o2, "m");

// 3 -- declared-class instance, chain ended on the instance.
const o3: any = new K();
Object.setPrototypeOf(o3, null);
show("3a classK+null data  ", o3, "a");
show("3b classK+null method", o3, "m");

// 4 -- object literal, chain ended. Builtins must go too.
const o4: any = {};
Object.setPrototypeOf(o4, null);
show("4a literal+null tostr", o4, "toString");
show("4b literal+null hasOw", o4, "hasOwnProperty");
show("4c literal+null ctor ", o4, "constructor");

// 5 -- born with no prototype. This ALWAYS worked, because a birth with no
// prototype has its own header bit; it is here so a fix that reaches for the
// class surface again cannot regress it unnoticed.
const o5: any = Object.create(null);
show("5a createnull tostr  ", o5, "toString");
show("5b createnull missing", o5, "a");

// 6 -- the CLASS's prototype is ended, the instance is never touched. Neither
// the receiver's guard nor the holder's can see this: the statement is one hop
// above the receiver and one below the answer.
Object.setPrototypeOf((K as any).prototype, null);
const o6: any = new K();
show("6a K.proto null tostr", o6, "toString");
show("6b K.proto null own  ", o6, "a");

// 7 -- the recorded prototype is itself an object with no prototype. The
// chain is two hops and ends at the second. `7c` is the sharpest row in the
// file: `a` lives on the prototype o7 was BORN with, which is no longer in
// its chain at all.
const n7: any = Object.create(null);
n7.z = 9;
const o7: any = Object.create(P);
Object.setPrototypeOf(o7, n7);
show("7a two-hop null tostr", o7, "toString");
show("7b two-hop null own  ", o7, "z");
show("7c two-hop null OLD  ", o7, "a");

// 8 -- ended, then pointed back at a real object. The chain is live again.
const o8: any = Object.create(P);
Object.setPrototypeOf(o8, null);
Object.setPrototypeOf(o8, P);
show("8a null then restored", o8, "a");

// 9 -- a plain-function prototype ended before construction. The own key is
// still found (it is ON the ended object, not above it); the builtin is not.
function D(this: any) {}
(D as any).prototype.a = 4;
Object.setPrototypeOf((D as any).prototype, null);
const o9: any = new (D as any)();
show("9a D.proto null own  ", o9, "a");
show("9b D.proto null tostr", o9, "toString");

// 10 -- `__proto__ = null` is the same statement spelled differently.
const o10: any = Object.create(P);
o10.__proto__ = null;
show("10a __proto__ = null ", o10, "a");
