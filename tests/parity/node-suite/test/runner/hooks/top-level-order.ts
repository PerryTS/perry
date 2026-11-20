import { after, afterEach, before, beforeEach, test } from "node:test";

before(() => console.log("hook:before"));
after(() => console.log("hook:after"));
beforeEach(() => console.log("hook:beforeEach"));
afterEach(() => console.log("hook:afterEach"));

test("first", () => console.log("body:first"));
test("second", () => console.log("body:second"));
