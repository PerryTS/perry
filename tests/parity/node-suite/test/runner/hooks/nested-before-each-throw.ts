import { after, afterEach, beforeEach, describe, test } from "node:test";

describe("nested hook failure", () => {
  beforeEach(() => {
    console.log("nested:beforeEach-throw");
    throw new Error("controlled beforeEach failure");
  });
  afterEach(() => {
    console.log("nested:afterEach-cleanup");
  });
  after(() => {
    console.log("nested:after-cleanup");
  });

  test("blocked child", () => {
    console.log("body:should-not-run");
  });
});
