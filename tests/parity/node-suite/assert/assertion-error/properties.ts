import assert from "node:assert";

try { assert.strictEqual(1, 2); } catch (err) {
  const e = err as { name?: string; code?: string; actual?: unknown; expected?: unknown; operator?: string; generatedMessage?: boolean };
  console.log("caught:", e.name, e.code);
  console.log("details:", e.actual, e.expected, e.operator, e.generatedMessage);
}
