// `Object.prototype.toLocaleString` is specified as `Invoke(O, "toString")`,
// which is `GetV(O, "toString")` -> `ToObject(O).[[Get]]("toString", O)`. That
// third argument is the RECEIVER, and it is the ORIGINAL primitive — not the
// wrapper object the property lookup walked through to find the property.
//
// The distinction is only observable when the prototype's `toString` is an
// ACCESSOR: a plain get resolves the getter against the prototype, so the
// getter's `this` was `Boolean.prototype` (an object) instead of the boolean.
// test262 pins this as
// `built-ins/Object/prototype/toLocaleString/primitive_this_value_getter.js`.
//
// Module code is strict, so a strict getter keeps the raw primitive `this`
// rather than a boxed wrapper — which is exactly the value being asserted.

Object.defineProperty(Boolean.prototype, "toString", {
  configurable: true,
  get: function (this: any) {
    const seen = typeof this;
    const value = this;
    return function () {
      return seen + ":" + String(value);
    };
  },
});

console.log("true:", (true as any).toLocaleString());
console.log("false:", (false as any).toLocaleString());

Object.defineProperty(String.prototype, "toString", {
  configurable: true,
  get: function (this: any) {
    const seen = typeof this;
    return function () {
      return seen;
    };
  },
});

console.log("string:", ("abc" as any).toLocaleString());

// A getter installed on `Symbol.prototype` sees the symbol itself.
Object.defineProperty(Symbol.prototype, "toString", {
  configurable: true,
  get: function (this: any) {
    const seen = typeof this;
    return function () {
      return seen;
    };
  },
});

console.log("symbol:", (Symbol("s") as any).toLocaleString());

// An object receiver is unaffected: the getter's `this` is the object itself,
// which is also what the receiver rule prescribes.
const proto = {
  get toString(this: any) {
    const seen = typeof this;
    const tag = this.tag;
    return function () {
      return seen + ":" + tag;
    };
  },
};
const obj: any = Object.create(proto);
obj.tag = "own";
console.log("object:", obj.toLocaleString());

// The ordinary DATA-property path must keep working — the accessor lookup is
// an addition to it, not a replacement.
Object.defineProperty(Boolean.prototype, "toString", {
  configurable: true,
  value: function (this: any) {
    return "data:" + typeof this;
  },
  writable: true,
});

console.log("data:", (true as any).toLocaleString());

// A present NON-CALLABLE must throw (`Invoke` -> `Call`), not collapse into the
// "native behavior still applies" fallback.
Object.defineProperty(String.prototype, "toString", {
  configurable: true,
  value: 42,
});
try {
  ("abc" as any).toLocaleString();
  console.log("data noncallable: missed");
} catch (error) {
  console.log("data noncallable:", error instanceof TypeError);
}

// The same distinction matters when an accessor resolves the value.
Object.defineProperty(Boolean.prototype, "toString", {
  configurable: true,
  get: function () {
    return null;
  },
});
try {
  (true as any).toLocaleString();
  console.log("accessor noncallable: missed");
} catch (error) {
  console.log("accessor noncallable:", error instanceof TypeError);
}

// TypedArray.prototype.toLocaleString performs the same Invoke operation for
// each numeric element, through the shared prototype resolver.
Object.defineProperty(Number.prototype, "toLocaleString", {
  configurable: true,
  value: 42,
});
try {
  new Int32Array([1]).toLocaleString();
  console.log("typed array noncallable: missed");
} catch (error) {
  console.log("typed array noncallable:", error instanceof TypeError);
}
