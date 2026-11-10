import test from "node:test";

test("subtest satisfies plan", async (t) => {
  t.plan(1);
  await t.test("planned child", () => {
    console.log("plan-child:body");
  });
  console.log("plan-parent:complete");
});
