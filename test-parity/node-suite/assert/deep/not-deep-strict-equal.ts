import assert from "node:assert";

assert.notDeepStrictEqual(1, 2);
try { assert.notDeepStrictEqual("same", "same"); } catch (err) { console.log("not deep strict same:", (err as { operator?: string }).operator); }
console.log("not deep strict ok");
