import assert from "node:assert";

for (const value of [false, 0, "problem"]) {
  try { assert.ifError(value); } catch (err) { console.log("ifError primitive:", (err as { operator?: string; actual?: unknown }).operator, (err as { actual?: unknown }).actual); }
}
