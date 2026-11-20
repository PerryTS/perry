import test from "node:test";

test("unmet assertion plan", (t) => {
  t.plan(1);
  console.log("plan:expected-one");
});
