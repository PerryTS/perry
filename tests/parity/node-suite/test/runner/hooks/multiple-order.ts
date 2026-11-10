import { after, before, test } from "node:test";

before(() => console.log("before:first"));
before(() => console.log("before:second"));
after(() => console.log("after:first"));
after(() => console.log("after:second"));

test("ordered hooks", () => console.log("hooks:body"));
