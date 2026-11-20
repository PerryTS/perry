import test from "node:test";

test("parent observes child failure", async (t) => {
  console.log("parent:before-failure");
  await t.test("failing child", () => {
    console.log("child:throwing");
    throw new Error("controlled child failure");
  });
  console.log("parent:after-failure");
});
