function add(a, b) {
  return a + b; // keeps its comment
}
const arrow = (x) => x * 2;
class Klass {
  method() { return "m"; }
}
globalThis.__cfsLoads = (globalThis.__cfsLoads || 0) + 1;
module.exports = {
  add,
  arrow,
  Klass,
  loads: globalThis.__cfsLoads,
  selfText: add.toString(),
  keysSeeFactory: Object.keys(module).some((k) => k.includes("factory")),
  factoryType: typeof module.__perry_cjs_factory,
  self: module,
};
