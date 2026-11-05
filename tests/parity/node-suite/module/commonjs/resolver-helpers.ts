import {
  Module,
  _findPath,
  _initPaths,
  _load,
  _nodeModulePaths,
  _preloadModules,
  _resolveFilename,
  _resolveLookupPaths,
} from "node:module";

const fixtureDir =
  process.cwd() + "/test-parity/node-suite/module/commonjs/fixtures";
const parent = new Module(fixtureDir + "/parent.js");
parent.filename = fixtureDir + "/parent.js";
parent.paths = Module._nodeModulePaths(fixtureDir);

function norm(value: unknown): string {
  return String(value)
    .split(fixtureDir)
    .join("<fixtures>")
    .split(process.cwd())
    .join("<cwd>");
}

function errorLine(label: string, fn: () => unknown) {
  try {
    console.log(`${label}:`, norm(fn()));
  } catch (error) {
    console.log(
      `${label}:`,
      (error as any).name,
      (error as any).code,
      String((error as any).message).includes("missing-local"),
    );
  }
}

console.log(
  "static helpers same:",
  Module._resolveFilename === _resolveFilename,
  Module._nodeModulePaths === _nodeModulePaths,
  Module._load === _load,
);
console.log(
  "_nodeModulePaths first:",
  norm(_nodeModulePaths(fixtureDir)[0]),
);
console.log(
  "_resolveLookupPaths relative:",
  norm(JSON.stringify(_resolveLookupPaths("./local-target", parent))),
);
console.log(
  "_resolveLookupPaths package prefix:",
  _resolveLookupPaths("some-pkg", parent)
    .slice(0, 3)
    .map(norm)
    .join("|"),
);
console.log("_resolveFilename fs:", _resolveFilename("fs", parent));
console.log("_resolveFilename node fs:", _resolveFilename("node:fs", parent));
console.log(
  "_resolveFilename local:",
  norm(_resolveFilename("./local-target", parent)),
);
console.log(
  "_resolveFilename local ext:",
  norm(_resolveFilename("./local-target.js", parent)),
);
errorLine("_resolveFilename missing", () =>
  _resolveFilename("./missing-local", parent),
);
console.log(
  "_findPath local:",
  norm(_findPath("./local-target", [fixtureDir])),
);
console.log(
  "_findPath missing:",
  String(_findPath("./missing-local", [fixtureDir])),
);
console.log("_findPath builtin:", String(_findPath("fs", parent.paths)));
console.log("_load builtin typeof:", typeof _load("fs", parent, false));
console.log("_preloadModules empty:", String(_preloadModules([])));
console.log("_initPaths:", String(_initPaths()));
console.log("globalPaths array after init:", Array.isArray(Module.globalPaths));
