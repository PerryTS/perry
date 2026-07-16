import { createRequire } from "node:module";

const req = createRequire(import.meta.url);
const resolved = req.resolve("./fixtures/data.json");
delete req.cache[resolved];
const first = req("./fixtures/data.json");
const second = req("./fixtures/data.json");
console.log("shape:", first.name, first.count, first.nested.ok);
console.log(
  "identity/cache:",
  first === second,
  req.cache[resolved]!.exports === first,
  req.cache[resolved]!.loaded,
);
first.count = 9;
console.log("mutation visible:", req("./fixtures/data.json").count);
delete req.cache[resolved];
console.log("delete reparses:", req("./fixtures/data.json").count);
delete req.cache[resolved];
