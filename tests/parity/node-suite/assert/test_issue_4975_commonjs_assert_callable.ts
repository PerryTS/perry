import { createRequire } from "node:module";

const dynamicRequire = createRequire(import.meta.url);
const assert = dynamicRequire("assert");

console.log(typeof assert);
assert(true);

try {
  assert(false, "expected failure");
} catch (error: any) {
  console.log(error.code);
}
