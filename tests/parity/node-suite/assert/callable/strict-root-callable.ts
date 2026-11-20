import assert from "node:assert";

assert.strict(true);
try {
  assert.strict(false, "nope");
} catch (e) {
  console.log((e as Error).message);
}
console.log("ok");
