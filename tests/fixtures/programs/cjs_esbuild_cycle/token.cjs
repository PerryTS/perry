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
var token_exports = {};
var getterReads = 0;
__export(token_exports, {
  getValue: () => (getterReads++, getValue),
  callThroughCycle: () => callThroughCycle,
  update: () => update,
  cycleView: () => cycleView,
  reads: () => reads
});
module.exports = __toCommonJS(token_exports);
globalThis.__esbuildCycleRecord = module;
var consumer = require("./consumer.cjs");
var offset = 40;
function getValue() { return offset + 2; }
function callThroughCycle() { return (0, consumer.read)(); }
function update() { getValue = function () { return 52; }; }
function cycleView() { return consumer.cycleView(); }
function reads() { return getterReads; }
