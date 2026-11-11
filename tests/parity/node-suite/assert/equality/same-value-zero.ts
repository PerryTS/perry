import assert from "node:assert";

assert.strictEqual(NaN, NaN);
try { assert.notStrictEqual(NaN, NaN); } catch (err) { console.log("nan not strict:", (err as { operator?: string }).operator); }
try { assert.strictEqual(0, -0); } catch (err) { console.log("zero sign strict:", (err as { operator?: string }).operator); }
assert.notStrictEqual(0, -0);
console.log("same value zero cases ok");
