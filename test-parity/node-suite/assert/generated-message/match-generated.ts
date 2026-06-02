import assert from "node:assert";

try { assert.match("abc", /z/); } catch (err) {
  const e = err as { generatedMessage?: boolean };
  console.log("match generated:", e.generatedMessage);
}
