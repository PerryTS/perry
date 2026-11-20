import test from "node:test";

test("named parent", async (t) => {
  console.log("context name:", t.name);
  await t.test("named child", (child) => {
    console.log("child context name:", child.name);
  });
});
