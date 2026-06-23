function logCall(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label, "ok", value);
  } catch (err: any) {
    console.log(label, "throw", err.name, err.message, err instanceof TypeError);
  }
}

function logTypeError(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label, "ok", value);
  } catch (err: any) {
    console.log(label, "throw", err.name, err instanceof TypeError);
  }
}

const obj = { x: 1 };
const custom = {
  toString() {
    return "custom";
  },
};
const nonCallableToString = { toString: 1 };
const nullPrototype = Object.create(null);

logCall("direct valueOf identity", () => obj.valueOf() === obj);
logCall("call valueOf identity", () => Object.prototype.valueOf.call(obj) === obj);
logCall("direct toLocaleString default", () => obj.toLocaleString());
logCall("direct toLocaleString custom", () => custom.toLocaleString());
logTypeError("direct toLocaleString noncallable", () => nonCallableToString.toLocaleString());
logCall("call toLocaleString default", () => Object.prototype.toLocaleString.call(obj));
logCall("call toLocaleString custom", () => Object.prototype.toLocaleString.call(custom));
logCall("call toLocaleString primitive", () => Object.prototype.toLocaleString.call(42));
logTypeError("call toLocaleString null-prototype", () =>
  Object.prototype.toLocaleString.call(nullPrototype),
);
logCall("call valueOf null", () => Object.prototype.valueOf.call(null));
logCall("call toLocaleString undefined", () => Object.prototype.toLocaleString.call(undefined));

// #3986: a user-defined own `valueOf` wins over the default Object.prototype
// .valueOf during ordinary member dispatch. `Object(x)` returns `x` unchanged,
// so `Object(x).valueOf()` must run x's own `valueOf` (test262 S9.9_A6). The
// explicit-base form `Object.prototype.valueOf.call(x)` must NOT consult the
// own property (it returns the receiver) — guarding against infinite recursion.
function MyValue(this: any, v: number) {
  this.value = v;
  this.valueOf = function () {
    return this.value;
  };
}
const mv = new (MyValue as any)(1);
const boxed: any = Object(mv);
logCall("Object(x) identity", () => boxed === mv);
logCall("Object(x) own valueOf", () => boxed.valueOf());
logCall("Object(x) coercion", () => boxed + 4);
logCall("own valueOf via member", () => mv.valueOf());
logCall("own valueOf base-call identity", () => Object.prototype.valueOf.call(boxed) === mv);
const litValueOf = {
  valueOf() {
    return 42;
  },
};
logCall("object literal own valueOf", () => litValueOf.valueOf());
logCall("object literal valueOf coercion", () => (litValueOf as any) * 2);
