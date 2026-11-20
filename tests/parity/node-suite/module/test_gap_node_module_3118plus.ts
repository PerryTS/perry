// node:module metadata + findPackageJSON parity (#3118, #3120).
// Byte-for-byte parity test against `node --experimental-strip-types`.
import * as module from "node:module";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { pathToFileURL } from "node:url";

// ── #3118: builtinModules metadata ──
console.log("builtinModules isArray:", Array.isArray(module.builtinModules));
console.log("builtinModules includes fs:", module.builtinModules.includes("fs"));
console.log("builtinModules includes path:", module.builtinModules.includes("path"));
console.log("builtinModules includes fs/promises:", module.builtinModules.includes("fs/promises"));

// ── #3118: isBuiltin (bare, node:-prefixed, subpath, unknown) ──
console.log("isBuiltin fs:", module.isBuiltin("fs"));
console.log("isBuiltin node:fs:", module.isBuiltin("node:fs"));
console.log("isBuiltin fs/promises:", module.isBuiltin("fs/promises"));
console.log("isBuiltin node:fs/promises:", module.isBuiltin("node:fs/promises"));
console.log("isBuiltin path:", module.isBuiltin("path"));
console.log("isBuiltin not-a-real-module:", module.isBuiltin("not-a-real-module"));
console.log("isBuiltin node:not-real:", module.isBuiltin("node:not-real"));

// ── #3120: findPackageJSON ──
console.log("findPackageJSON typeof:", typeof module.findPackageJSON);
const dir: string = fs.mkdtempSync(path.join(os.tmpdir(), "gap-fpj-"));
fs.mkdirSync(path.join(dir, "src"));
fs.writeFileSync(path.join(dir, "package.json"), JSON.stringify({ name: "probe-pkg" }));
fs.writeFileSync(path.join(dir, "src", "entry.js"), "export {};\n");

const r1 = module.findPackageJSON(".", pathToFileURL(path.join(dir, "src", "entry.js")));
console.log("fpj r1 isString:", typeof r1 === "string");
console.log("fpj r1 basename:", path.basename(r1 as string));
console.log("fpj r1 in dir:", (r1 as string).includes("gap-fpj-"));

const r2 = module.findPackageJSON("./src/entry.js", pathToFileURL(dir + path.sep));
console.log("fpj r2 basename:", path.basename(r2 as string));
console.log("fpj r2 in dir:", (r2 as string).includes("gap-fpj-"));

// Missing specifier throws ERR_MISSING_ARGS.
try {
  // @ts-ignore - intentionally calling with no args
  module.findPackageJSON();
  console.log("fpj missing: no throw");
} catch (e) {
  console.log("fpj missing code:", (e as { code?: string }).code);
}
