import { createRequire } from "node:module";

const req = createRequire(import.meta.url);
const required = req("./fixtures/cjs-object.cjs");
const first = await import("./fixtures/cjs-object.cjs");
const second = await import("./fixtures/cjs-object.cjs");
console.log("namespace cached:", first === second);
console.log(
  "default/marker:",
  first.default === required,
  first["module.exports"] === required,
);
console.log("named/shared:", first.named, first.shared === required.shared);
console.log(
  "require cache:",
  req.cache[req.resolve("./fixtures/cjs-object.cjs")]!.exports === required,
);
