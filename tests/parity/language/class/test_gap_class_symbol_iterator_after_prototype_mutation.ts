// #11692: a class's `[Symbol.iterator]()` method must keep driving
// iteration after a prototype mutation elsewhere. #11667 derived "this
// declared prototype member was deleted" from "its string key is not on the
// prototype object once the prototype guards are invalidated"; a
// `[Symbol.iterator]` method is dispatched under the synthetic name
// `@@iterator` but lives under its symbol key, so the first
// `Object.setPrototypeOf` anywhere made `Array.from(x)` / `for...of` throw
// `@@iterator is not a function` (mongodb's parseOptions, via
// mongodb-connection-string-url re-parenting whatwg-url's URLSearchParams).

class Impl {
  _list: any[] = [["x", "1"], ["y", "2"]];
  [Symbol.iterator]() {
    return this._list[Symbol.iterator]();
  }
}

class Params {
  keys() {
    return ["k1", "k2"][Symbol.iterator]();
  }
}
class Upper extends Params {}

const impl = new Impl();
console.log("before:", JSON.stringify(Array.from(impl)));

const p: any = new Params();
Object.setPrototypeOf(p, Upper.prototype);

console.log("after setPrototypeOf:", JSON.stringify(Array.from(impl)));
for (const [k, v] of impl) console.log("for-of:", k, v);
console.log("spread:", [...impl].length);
for (const k of p.keys()) console.log("re-parented keys:", k);

// The mongodb-connection-string-url shape: a wrapper class built around a
// runtime parent re-parents an existing instance; its iterator forwards to the
// parent's through `super`.
const implKey = Symbol("impl");
class SP {
  constructor() {
    (this as any)[implKey] = new Impl();
  }
  entries() {
    return Array.from((this as any)[implKey])[Symbol.iterator]();
  }
  [Symbol.iterator]() {
    return this.entries();
  }
}
function caseInsensitive(Ctor: any) {
  return class CI extends Ctor {
    entries() {
      return super.entries();
    }
  };
}
const sp: any = new SP();
Object.setPrototypeOf(sp, caseInsensitive(SP).prototype);
for (const [k, v] of sp) console.log("wrapped:", k, v);

// A method literally named "@@iterator" is an ordinary string key.
class Literal {
  ["@@iterator"]() {
    return "literal";
  }
}
const lit: any = new Literal();
console.log("literal:", lit["@@iterator"]());
delete (Literal.prototype as any)["@@iterator"];
console.log("literal deleted:", typeof lit["@@iterator"]);
