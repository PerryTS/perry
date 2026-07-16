import { after, test } from "node:test";

after(() => {
  console.log("cleanup:after");
});

test("throws", () => {
  console.log("body:before-throw");
  throw new Error("controlled hook cleanup failure");
});
