import test from "node:test";

console.log("missing:before");
test("test without callback");
console.log("missing:after");
