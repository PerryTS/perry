import { mock } from "node:test";

const fn = mock.fn();
console.log("initial:", fn.mock.callCount(), fn.mock.calls.length);
console.log("result:", fn("a", 2));
const call = fn.mock.calls[0];
console.log(
  "record:",
  JSON.stringify(call.arguments),
  call.result === undefined,
  call.error === undefined,
  call.this === undefined,
  call.target === undefined,
);
mock.restoreAll();
