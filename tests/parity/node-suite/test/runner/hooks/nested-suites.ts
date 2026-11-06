import { after, afterEach, before, beforeEach, describe, it } from "node:test";

before(() => console.log("outer:before"));
after(() => console.log("outer:after"));
beforeEach(() => console.log("outer:beforeEach"));
afterEach(() => console.log("outer:afterEach"));

describe("nested hooks", () => {
  before(() => console.log("inner:before"));
  after(() => console.log("inner:after"));
  beforeEach(() => console.log("inner:beforeEach"));
  afterEach(() => console.log("inner:afterEach"));

  it("child", () => console.log("body:child"));
});
