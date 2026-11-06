import { mock } from "node:test";

const target = {
  base: 4,
  add(value: number) {
    return this.base + value;
  },
};

const method = mock.method(target, "add");
console.log("method result:", target.add(3));
const call = method.mock.calls[0];
console.log("method record:", call.this === target, JSON.stringify(call.arguments), call.result);
method.mock.restore();
