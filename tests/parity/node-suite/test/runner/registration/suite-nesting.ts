import { describe, it } from "node:test";

describe("outer suite", () => {
  console.log("suite:body");
  describe("inner suite", () => {
    it("nested test", () => {
      console.log("test:body");
    });
  });
});
