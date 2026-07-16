import test from "node:test";

test("parent completion", async (t) => {
  console.log("parent:start");
  const child = t.test("async child", async () => {
    console.log("child:start");
    await Promise.resolve();
    console.log("child:end");
  });
  console.log("parent:registered-child");
  await child;
  console.log("parent:after-child");
});
