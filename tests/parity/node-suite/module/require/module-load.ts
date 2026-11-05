import { createRequire, Module } from "node:module";
import { fileURLToPath } from "node:url";

const req = createRequire(import.meta.url);
const filename = req.resolve("./fixtures/metadata.cjs");
const instance = new Module("manual-id");
instance.filename = filename;
instance.paths = Module._nodeModulePaths(
  fileURLToPath(new URL("./fixtures/", import.meta.url)),
);
console.log(
  "before:",
  instance.loaded,
  Object.keys(instance.exports).length,
  instance.children.length,
);
console.log("return:", String(instance.load(filename)));
console.log(
  "after:",
  instance.loaded,
  instance.id,
  instance.filename === filename,
  (instance.exports as any).loadedDuringEvaluation,
);
console.log("cache independent:", req.cache[filename] === undefined);
try {
  instance.load(filename);
  console.log("second load: no throw");
} catch (error) {
  console.log(
    "second load:",
    (error as any).name,
    (error as any).code ?? "no-code",
  );
}
