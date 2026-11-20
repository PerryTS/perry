import { mock } from "node:test";

function callKeys(call: any): string {
  return Object.keys(call).sort().join(",");
}

const fn = mock.fn(function add(this: any, a: number, b: number) {
  return this.base + a + b;
});
const fnResult = fn.call({ label: "fn-this", base: 5 }, 2, 3);
const fnCall = fn.mock.calls[0];
console.log(
  "fn result:",
  fnResult,
  fnCall.this.label,
  fnCall.result,
  typeof fnCall.stack,
  callKeys(fnCall),
);

const methodTarget = {
  label: "method-this",
  value: 4,
  add(n: number) {
    return this.value + n;
  },
};
const method = mock.method(methodTarget, "add", function (this: any, n: number) {
  return this.value * n;
});
console.log(
  "method result:",
  methodTarget.add(3),
  method.mock.calls[0].this.label,
  method.mock.calls[0].result,
);
method.mock.restore();
console.log("method restore:", methodTarget.add(3));

const accessorTarget = {
  label: "accessor-this",
  _value: 2,
  get value() {
    return this._value + 1;
  },
  set value(v: number) {
    this._value = v * 2;
  },
};

const getter = mock.getter(accessorTarget, "value", function (this: any) {
  return this._value + 10;
});
const getterDesc = Object.getOwnPropertyDescriptor(accessorTarget, "value")!;
console.log(
  "getter desc:",
  typeof getterDesc.get,
  typeof getterDesc.set,
  getterDesc.enumerable,
  getterDesc.configurable,
);
console.log(
  "getter result:",
  accessorTarget.value,
  getter.mock.callCount(),
  getter.mock.calls[0].this.label,
  getter.mock.calls[0].result,
);
getter.mock.restore();
console.log("getter restore:", accessorTarget.value);

const setter = mock.setter(accessorTarget, "value", function (this: any, v: number) {
  this._value = v * 3;
});
const setterDesc = Object.getOwnPropertyDescriptor(accessorTarget, "value")!;
console.log(
  "setter desc:",
  typeof setterDesc.get,
  typeof setterDesc.set,
  setterDesc.enumerable,
  setterDesc.configurable,
);
accessorTarget.value = 4;
console.log(
  "setter result:",
  accessorTarget._value,
  setter.mock.callCount(),
  setter.mock.calls[0].arguments[0],
  setter.mock.calls[0].this.label,
  setter.mock.calls[0].result === undefined ? "undefined" : setter.mock.calls[0].result,
);
mock.restoreAll();
accessorTarget.value = 5;
console.log("restoreAll setter:", accessorTarget._value);
