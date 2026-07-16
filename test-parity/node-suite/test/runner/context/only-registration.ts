import test from "node:test";

test("regular sibling", () => {
  console.log("only:regular");
});

test.only("marked only", () => {
  console.log("only:marked");
});
