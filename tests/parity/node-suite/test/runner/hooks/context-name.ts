import { after, before, test } from "node:test";

before((t) => console.log("before context:", t.name));
after((t) => console.log("after context:", t.name));

test("hook context body", () => console.log("hook-context:body"));
