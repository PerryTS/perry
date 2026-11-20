import { after, before, test } from "node:test";

before(() => {
  console.log("hook:before-throw");
  throw new Error("controlled before failure");
});

after(() => {
  console.log("hook:after-cleanup");
});

test("blocked body", () => {
  console.log("body:should-not-run");
});
