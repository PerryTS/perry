import { mock } from "node:test";

let inside = -1;
const fn = mock.fn(() => {
  inside = fn.mock.callCount();
  return "value";
});

console.log("call result:", fn());
console.log("counts:", inside, fn.mock.callCount());
mock.restoreAll();
