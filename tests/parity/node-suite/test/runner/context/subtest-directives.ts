import test from "node:test";

test("directive parent", async (t) => {
  await t.test("option skip child", { skip: "option reason" }, () => {
    console.log("skip-child:should-not-run");
  });
  await t.test("option todo child", { todo: "todo reason" }, () => {
    console.log("todo-child:ran");
  });
  console.log("directive-parent:complete");
});
