const first = require("./fixtures/ro-run-once/reload.cjs");
const key = Symbol("reload");
const holder = {
  get [key]() {
    const m = process.getBuiltinModule("module");
    const r = m.createRequire(__filename);
    const path = r.resolve("./fixtures/ro-run-once/reload.cjs");
    delete m._cache[path];
    return r(path);
  }
};
const second = holder[key];
console.log("identity", first !== second, first.Leaf !== second.Leaf, first.Leaf.prototype !== second.Leaf.prototype);
first.Leaf.prototype.read = () => 71;
console.log("values", first.make().read(), second.make().read());
