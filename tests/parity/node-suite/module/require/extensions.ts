import { createRequire, Module } from "node:module";

const req = createRequire(import.meta.url);
console.log("shared:", req.extensions === Module._extensions);
console.log("keys:", Object.keys(req.extensions).sort().join(","));
const customPath = req.resolve("./fixtures/custom.ext");
delete req.cache[customPath];
const previous = req.extensions[".ext"];
let calls = 0;
req.extensions[".ext"] = (module, filename) => {
  calls++;
  req.extensions[".js"]!(module, filename);
};
try {
  console.log(
    "custom:",
    req("./fixtures/custom.ext").extension,
    calls,
    req.cache[customPath]!.loaded,
  );
} finally {
  delete req.cache[customPath];
  if (previous) req.extensions[".ext"] = previous;
  else delete req.extensions[".ext"];
}
console.log("restored:", req.extensions[".ext"] === previous);
