import test from "node:test";

test("runtime skip", (t) => {
  console.log("runtime-skip:before");
  t.skip("not applicable");
  console.log("runtime-skip:after");
});

test("runtime todo", (t) => {
  console.log("runtime-todo:before");
  t.todo("not implemented");
  console.log("runtime-todo:after");
});
