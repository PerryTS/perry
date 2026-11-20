import { mock } from "node:test";

const expected = new Error("controlled mock failure");
const fn = mock.fn(function (this: any, value: number) {
  if (value < 0) throw expected;
  return this.offset + value;
});

console.log("success:", fn.call({ label: "receiver", offset: 4 }, 3));
try {
  fn(-1);
} catch (error) {
  console.log("caught:", error === expected);
}

const success = fn.mock.calls[0];
const failure = fn.mock.calls[1];
console.log(
  "success record:",
  JSON.stringify(success.arguments),
  success.this.label,
  success.result,
  success.error === undefined,
  success.target === undefined,
);
console.log(
  "failure record:",
  JSON.stringify(failure.arguments),
  failure.result === undefined,
  failure.error === expected,
  typeof failure.stack,
);
mock.restoreAll();
