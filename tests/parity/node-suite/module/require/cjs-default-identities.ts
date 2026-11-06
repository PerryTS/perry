import moduleDefault, * as moduleNS from "node:module";
import pathDefault, * as pathNS from "node:path";
import osDefault, * as osNS from "node:os";
import hooksDefault, * as hooksNS from "node:async_hooks";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const mod = require("module");
const prefixedMod = require("node:module");
const path = require("path");
const prefixedPath = require("node:path");
const os = require("os");
const prefixedOs = require("node:os");
const hooks = require("async_hooks");
const prefixedHooks = require("node:async_hooks");

console.log("module type:", typeof mod);
console.log("module namespace/default:", mod === moduleNS, mod === moduleNS.default);
console.log("module imports:", mod === prefixedMod, mod === moduleDefault, mod === moduleNS.Module);
console.log("module callable:", mod.createRequire === createRequire);
console.log("path namespace/default:", path === pathNS, path === pathNS.default);
console.log("path imports:", path === prefixedPath, path === pathDefault);
console.log("path callable:", path.join === pathNS.join);
console.log("os namespace/default:", os === osNS, os === osNS.default);
console.log("os imports:", os === prefixedOs, os === osDefault);
console.log("os callable:", os.endianness === osNS.endianness);
console.log("hooks namespace/default:", hooks === hooksNS, hooks === hooksNS.default);
console.log("hooks imports:", hooks === prefixedHooks, hooks === hooksDefault);
console.log("hooks callable:", hooks.createHook === hooksNS.createHook);

const join = path.join;
console.log("path calls:", path.join("/one", "two"), join("/one", "two"));
