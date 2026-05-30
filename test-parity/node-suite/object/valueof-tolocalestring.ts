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
