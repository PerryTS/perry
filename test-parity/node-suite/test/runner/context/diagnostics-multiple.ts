import test from "node:test";

test("multiple diagnostics", (t) => {
  console.log("diagnostic:before");
  t.diagnostic("first message");
  t.diagnostic("second message");
  console.log("diagnostic:after");
});
