// A CommonJS module shaped like next/dist/shared/lib/constants.js: a dead
// `0 && (module.exports = { NAME: null, ... })` hint block, then the live
// getters installed by `_export(exports, { NAME: function() { ... } })`.
// The consumer must see the getters' values, never the hint's nulls, and a
// literal from that module must work as a `path.join` operand (the Next.js
// server joined distDir with BUILD_ID_FILE and read the `.next` directory).
import * as path from "path";
import constants from "./_helpers/cjs_export_hint_constants.cjs";

console.log("BUILD_ID_FILE", JSON.stringify(constants.BUILD_ID_FILE));
console.log("BLOCKED_PAGES", JSON.stringify(constants.BLOCKED_PAGES));
console.log("CONFIG_FILES", JSON.stringify(constants.CONFIG_FILES));
console.log("keys", JSON.stringify(Object.keys(constants)));
const desc = Object.getOwnPropertyDescriptor(constants, "BUILD_ID_FILE");
console.log("getter", typeof desc?.get, desc?.enumerable);
const distDir = [".", "next"].join("");
console.log("join", path.join(distDir, constants.BUILD_ID_FILE));
console.log("join-literal", path.join(".next", constants.BUILD_ID_FILE));
