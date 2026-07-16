import { createRequire } from "node:module";

const req = createRequire(import.meta.url);
const aPath = req.resolve("./fixtures/cycle-a.cjs");
const bPath = req.resolve("./fixtures/cycle-b.cjs");
delete req.cache[aPath];
delete req.cache[bPath];
const a = req("./fixtures/cycle-a.cjs");
const b = req("./fixtures/cycle-b.cjs");
console.log(
  "exports:",
  a.name,
  a.sawB,
  a.bSawAReady,
  a.ready,
  b.name,
  b.sawA,
  b.sawAReady,
);
console.log(
  "cache loaded:",
  req.cache[aPath]!.loaded,
  req.cache[bPath]!.loaded,
);
console.log(
  "cycle children:",
  req.cache[aPath]!.children[0] === req.cache[bPath],
  req.cache[bPath]!.children[0] === req.cache[aPath],
);
delete req.cache[aPath];
delete req.cache[bPath];
