import * as moduleNS from "node:module";

for (
  const key of [
    "createRequire",
    "enableCompileCache",
    "findPackageJSON",
    "findSourceMap",
    "flushCompileCache",
    "getCompileCacheDir",
    "getSourceMapsSupport",
    "isBuiltin",
    "register",
    "registerHooks",
    "runMain",
    "setSourceMapsSupport",
    "stripTypeScriptTypes",
    "syncBuiltinESMExports",
  ] as const
) {
  const fn = moduleNS[key] as Function;
  console.log(
    key,
    typeof fn,
    fn.name,
    fn.length,
    Object.prototype.hasOwnProperty.call(fn, "prototype"),
  );
}
