import { ok, strictEqual, notStrictEqual } from "node:assert";

ok("value");
strictEqual(1 + 1, 2);
notStrictEqual({}, {});
console.log("named ok");
