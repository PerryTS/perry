import ModuleDefault, * as moduleNS from "node:module";

const keys = [
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
] as const;

console.log(
  "all identities:",
  keys.every((key) => (ModuleDefault as any)[key] === moduleNS[key]),
);
console.log(
  "default extras:",
  typeof ModuleDefault.wrap,
  Array.isArray(ModuleDefault.wrapper),
);
console.log(
  "own names include extras:",
  ["wrap", "wrapper", "_readPackage", "_stat"].every((key) =>
    Object.prototype.hasOwnProperty.call(ModuleDefault, key)
  ),
);
