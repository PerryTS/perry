import assert from "node:assert";

assert(true);
try {
  assert(false, "nope");
} catch (e) {
  console.log((e as Error).message);
}
console.log("ok");
