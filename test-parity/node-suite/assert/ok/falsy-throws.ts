import assert from "node:assert";

for (const value of [false, 0, "", null, undefined, NaN]) {
  try { assert.ok(value); } catch (err) { console.log("throws:", (err as Error).name, (err as { code?: string }).code); }
}
