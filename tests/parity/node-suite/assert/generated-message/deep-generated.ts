import assert from "node:assert";

try { assert.deepStrictEqual({ a: 1 }, { a: 2 }); } catch (err) {
  const e = err as { generatedMessage?: boolean };
  console.log("deep generated:", e.generatedMessage);
}
