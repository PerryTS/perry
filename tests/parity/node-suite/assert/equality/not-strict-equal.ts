import assert from "node:assert";

assert.notStrictEqual(1, 2);
assert.notStrictEqual({}, {});
try { assert.notStrictEqual("x", "x"); } catch (err) { console.log("not strict same:", (err as { operator?: string }).operator); }
try { assert.notStrictEqual(NaN, NaN); } catch (err) { console.log("not strict nan:", (err as { operator?: string }).operator); }
