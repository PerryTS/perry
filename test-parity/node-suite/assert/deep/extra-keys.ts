import assert from "node:assert";

try { assert.deepStrictEqual({ a: 1 }, { a: 1, b: 2 }); } catch (err) {
  const e = err as { name?: string; code?: string };
  console.log("missing key:", e.name, e.code);
}
try { assert.deepStrictEqual({ a: 1, b: 2 }, { a: 1 }); } catch (err) {
  const e = err as { name?: string; code?: string };
  console.log("extra key:", e.name, e.code);
}
