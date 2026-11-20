import test from "node:test";

console.log("registration:start");
test("top level", () => {
  console.log("body:top level");
});
console.log("registration:end");
