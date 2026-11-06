import { createRequire } from "node:module";

const reqA = createRequire(import.meta.url);
const reqB = createRequire(new URL("./other-base.cjs", import.meta.url));
const resolved = reqA.resolve("./fixtures/value.cjs");
delete reqA.cache[resolved];
const first = reqA("./fixtures/value.cjs");
const second = reqB("./fixtures/value.cjs");
console.log(
  "shared identity/cache:",
  first === second,
  reqA.cache === reqB.cache,
  reqA.cache[resolved]!.exports === first,
);
console.log(
  "cache metadata:",
  reqA.cache[resolved]!.loaded,
  reqA.cache[resolved]!.filename === resolved,
  Array.isArray(reqA.cache[resolved]!.children),
);
delete reqA.cache[resolved];
const third = reqA("./fixtures/value.cjs");
console.log(
  "delete reload:",
  third !== first,
  first.loads,
  third.loads,
  reqA.cache[resolved]!.exports === third,
);
delete reqA.cache[resolved];
delete (globalThis as any).__moduleValueLoads;
