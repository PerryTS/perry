import { afterEach, beforeEach, test } from "node:test";

beforeEach(() => console.log("skip:beforeEach"));
afterEach(() => console.log("skip:afterEach"));

test("runtime skip cleanup", (t) => {
  console.log("skip:body");
  t.skip("controlled runtime skip");
});
