var __defProp = Object.defineProperty;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from) => {
  for (let key of __getOwnPropNames(from))
    __defProp(to, key, { get: () => from[key], enumerable: true });
  return to;
};
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);
var consumer_exports = {};
__export(consumer_exports, { read: () => read, cycleView: () => cycleView });
module.exports = __toCommonJS(consumer_exports);
var token = require("./token.cjs");
// Inspect both views while token's wrapper is still running. Neither
// descriptor inspection nor retaining the namespace should invoke a getter.
var record = globalThis.__esbuildCycleRecord;
var cachedGetter = record ? typeof Object.getOwnPropertyDescriptor(record.exports, "getValue")?.get : "missing-record";
var requiredGetter = token ? typeof Object.getOwnPropertyDescriptor(token, "getValue")?.get : "missing-exports";
var sameExports = record ? token === record.exports : false;
function read() { return (0, token.getValue)(); }
function cycleView() { return cachedGetter + ":" + requiredGetter + ":" + sameExports; }
