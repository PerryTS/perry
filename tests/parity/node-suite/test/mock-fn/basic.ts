import { mock } from "node:test";

const fn = mock.fn((x: number) => x + 1);
console.log("first:", fn(2), fn.mock.callCount(), JSON.stringify(fn.mock.calls[0].arguments));

const withImplementation = mock.fn(
  (x: number) => x * 2,
  (x: number) => x * 3,
);
console.log("implementation:", withImplementation(4), withImplementation.mock.callCount());

fn.mock.mockImplementationOnce((x: number) => x + 10);
console.log("once:", fn(1), fn(1), fn.mock.callCount());

fn.mock.resetCalls();
console.log("after reset:", fn.mock.callCount());

const obj = {
  value: 2,
  add(x: number) {
    return this.value + x;
  },
};

const replacement = mock.method(obj, "add", function (x: number) {
  return obj.value * x;
});
console.log(
  "method:",
  obj.add(3),
  replacement.mock.callCount(),
  JSON.stringify(replacement.mock.calls[0].arguments),
);
replacement.mock.restore();
console.log("method restore:", obj.add(3));

mock.property(obj, "value", 5);
console.log("property:", obj.value);
mock.restoreAll();
console.log("restoreAll:", obj.value);
