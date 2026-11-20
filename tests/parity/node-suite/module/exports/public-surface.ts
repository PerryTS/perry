import ModuleDefault, * as moduleNS from "node:module";

const expected = [
  "Module",
  "SourceMap",
  "_cache",
  "_extensions",
  "_findPath",
  "_initPaths",
  "_load",
  "_nodeModulePaths",
  "_pathCache",
  "_preloadModules",
  "_resolveFilename",
  "_resolveLookupPaths",
  "builtinModules",
  "constants",
  "createRequire",
  "default",
  "enableCompileCache",
  "findPackageJSON",
  "findSourceMap",
  "flushCompileCache",
  "getCompileCacheDir",
  "getSourceMapsSupport",
  "globalPaths",
  "isBuiltin",
  "register",
  "registerHooks",
  "runMain",
  "setSourceMapsSupport",
  "stripTypeScriptTypes",
  "syncBuiltinESMExports",
];

console.log("keys:", JSON.stringify(Object.keys(moduleNS).sort()));
console.log(
  "exact:",
  JSON.stringify(Object.keys(moduleNS).sort()) === JSON.stringify(expected),
);
console.log(
  "identity:",
  ModuleDefault === moduleNS.Module,
  moduleNS.default === moduleNS.Module,
);
console.log("tag:", Object.prototype.toString.call(moduleNS));
console.log("extensible:", Object.isExtensible(moduleNS));
