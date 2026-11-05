// Issue #6233 — a module-scope user class legally shadows a same-named
// global (`class Symbol extends Base {}` … `new Symbol()`), but the built-in
// constructor arms in `lower_new` fired by NAME alone, so the `new` bound to
// the intrinsic instead of the user class. Depending on the name this either
// threw ("Symbol is not a constructor", the Proxy target TypeError, the
// WeakRef/FinalizationRegistry/AggregateError argument validation), or
// silently constructed the wrong object (native Map/Set/Date/boxed
// primitive/Error/typed array — field initializers never ran, instanceof
// against the user class was false). Found via effect's SchemaAST.ts, which
// declares AST node classes named `Symbol` and `BigInt` and constructs them
// at module init.

class Base {
  describe(): string {
    return "base";
  }
}

// The exact shape from the issue / effect's SchemaAST.ts.
class Symbol extends Base {
  readonly _tag = "Symbol";
}
const node = new Symbol();
console.log("constructed:", node._tag, node instanceof Symbol, node.describe());

// Crash bucket: bound to a non-constructible intrinsic or the intrinsic's
// argument validation.
class BigInt extends Base {
  readonly tag = "BigInt";
}
const big = new BigInt();
console.log("BIGINT", big.tag, big instanceof BigInt);

class Proxy {
  readonly target: string;
  constructor(target: string) {
    this.target = target;
  }
  who(): string {
    return "user-proxy-" + this.target;
  }
}
const prox = new Proxy("t");
console.log("PROXY", prox.who(), prox instanceof Proxy);

class WeakRef {
  readonly inner: string;
  constructor(inner: string) {
    this.inner = inner;
  }
  // Reserved native method name — must dispatch to the USER method, not the
  // WeakRef intrinsic fast path (the weak-locals pre-scan tagged the binding
  // by constructor name alone).
  deref(): string {
    return "user-deref-" + this.inner;
  }
}
const wref = new WeakRef("w");
console.log("WEAKREF", wref.deref(), wref instanceof WeakRef);

class FinalizationRegistry {
  readonly tag = "FinalizationRegistry";
}
const finreg = new FinalizationRegistry();
console.log("FINREG", finreg.tag, finreg instanceof FinalizationRegistry);

class AggregateError {
  readonly tag = "AggregateError";
}
const agg = new AggregateError();
console.log("AGG", agg.tag, agg instanceof AggregateError);

// Wrong-object bucket: the native constructor ran instead of the user class,
// so field initializers never executed and instanceof was false.
class Map {
  private store: Record<string, number> = {};
  // Reserved native Map method names — must hit the user methods.
  set(k: string, v: number): void {
    this.store[k] = v;
  }
  get(k: string): number {
    return this.store[k] ?? -1;
  }
}
const map = new Map();
map.set("a", 41);
console.log("MAP", map.get("a"), map.get("missing"), map instanceof Map);

class Set {
  readonly tag = "Set";
}
const set = new Set();
console.log("SET", set.tag, set instanceof Set);

class Date {
  readonly tag = "Date";
}
const date = new Date();
console.log("DATE", date.tag, date instanceof Date);

class Number {
  readonly tag = "Number";
}
const num = new Number();
console.log("NUMBER", num.tag, num instanceof Number);

class String {
  readonly tag = "String";
}
const str = new String();
console.log("STRING", str.tag, str instanceof String);

class Boolean {
  readonly tag = "Boolean";
}
const bool = new Boolean();
console.log("BOOLEAN", bool.tag, bool instanceof Boolean);

class Error {
  readonly tag = "Error";
}
const err = new Error();
console.log("ERROR", err.tag, err instanceof Error);

class TypeError {
  readonly tag = "TypeError";
}
const terr = new TypeError();
console.log("TYPEERROR", terr.tag, terr instanceof TypeError);

class Uint8Array {
  readonly tag = "Uint8Array";
}
const u8 = new Uint8Array();
console.log("UINT8", u8.tag, u8 instanceof Uint8Array);

class Int32Array {
  readonly tag = "Int32Array";
}
const i32 = new Int32Array();
console.log("INT32", i32.tag, i32 instanceof Int32Array);

// Constructor arguments still flow to the user class (the RegExp arm used to
// consume them for the intrinsic's literal/dynamic paths).
class RegExp {
  readonly pattern: string;
  constructor(pattern: string) {
    this.pattern = pattern;
  }
}
const rx = new RegExp("user-pattern");
console.log("REGEXP", rx.pattern, rx instanceof RegExp);

// Previously-working names — must stay correct.
class Widget extends Base {
  readonly tag = "Widget";
}
const widget = new Widget();
console.log("WIDGET", widget.tag, widget instanceof Widget);

class Object {
  readonly tag = "Object";
}
const obj = new Object();
console.log("OBJECT", obj.tag, obj instanceof Object);

class Array {
  readonly tag = "Array";
}
const arr = new Array();
console.log("ARRAY", arr.tag, arr instanceof Array);

class Promise {
  readonly tag = "Promise";
}
const prom = new Promise();
console.log("PROMISE", prom.tag, prom instanceof Promise);

class Function {
  readonly tag = "Function";
}
const func = new Function();
console.log("FUNCTION", func.tag, func instanceof Function);
