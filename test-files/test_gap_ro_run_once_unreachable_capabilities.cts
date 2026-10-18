const { Base } = require("./fixtures/ro-run-once/base.cjs");
function unusedLoader() {
  const m = process.getBuiltinModule("module");
  const r = m.createRequire(__filename);
  const path = r.resolve("./fixtures/ro-run-once/reload.cjs");
  delete m._cache[path];
  return r(path);
}
function unusedDynamic(source) {
  const key = ["con", "structor"].join("");
  return (() => 0)[key](source);
}
class Leaf extends Base { read() { return super.read() + 1; } }
const first = new Leaf();
const second = new Leaf();
console.log("same", first.constructor === Leaf, second.constructor === Leaf);
Leaf.prototype.read = () => 71;
console.log("values", first.read(), second.read());
