import assert from "node:assert";

assert.strictEqual(1, 1);
assert.strictEqual("x", "x");
assert.strictEqual(null, null);
try { assert.strictEqual(1, "1" as unknown as number); } catch (err) { console.log("strict mismatch:", (err as { operator?: string }).operator, (err as { actual?: unknown }).actual, (err as { expected?: unknown }).expected); }
try { assert.strictEqual(NaN, NaN); console.log("nan same"); } catch { console.log("nan throws"); }
