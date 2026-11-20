import test from "node:test";

test("zero assertion plan", (t) => {
  t.plan(0);
  console.log("plan:zero");
});
