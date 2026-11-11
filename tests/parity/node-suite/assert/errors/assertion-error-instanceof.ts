import assert from "node:assert";

try { assert.strictEqual(1, 2); } catch (err) {
  const e = err as { name?: string; code?: string };
  console.log("name:", e.name === "AssertionError");
  console.log("code:", e.code === "ERR_ASSERTION");
  console.log("err instanceof Error:", err instanceof Error);
}
