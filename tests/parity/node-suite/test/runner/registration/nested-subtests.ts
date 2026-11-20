import test from "node:test";

test("parent", async (t) => {
  console.log("parent:body:start");
  await t.test("child", () => {
    console.log("child:body");
  });
  console.log("parent:body:end");
});
