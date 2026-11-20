import { createRequire } from "node:module";

const req = createRequire(import.meta.url);
const slash = (value: string) => value.replaceAll("\\", "/");
const resolved = req.resolve("./fixtures/metadata.cjs");
delete req.cache[resolved];
const value = req("./fixtures/metadata.cjs");
const record = req.cache[resolved]!;
console.log(
  "identity:",
  value.id === resolved,
  value.filename === resolved,
  record.id === resolved,
  record.filename === resolved,
);
console.log(
  "directory/path:",
  slash(value.path).endsWith("/require/fixtures"),
  slash(value.paths[0]).endsWith("/require/fixtures/node_modules"),
);
console.log("evaluation/loaded:", value.loadedDuringEvaluation, record.loaded);
console.log(
  "require/parent:",
  value.requireIdentity,
  typeof value.parentId,
  record.parent === undefined || typeof record.parent.id === "string",
);
delete req.cache[resolved];
