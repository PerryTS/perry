import { afterEach, test } from "node:test";

afterEach(() => {
  console.log("hook:afterEach-cleanup");
});

test("body throws", () => {
  console.log("body:before-throw");
  throw new Error("controlled body failure");
});
