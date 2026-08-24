import adapter from "./barrel.js";

const out = {};
out.keys = Object.keys(adapter).join(",");
const setupDescriptor = Object.getOwnPropertyDescriptor(adapter, "setup");
out.descriptor = [
  setupDescriptor.enumerable,
  setupDescriptor.writable,
  setupDescriptor.configurable,
].join(",");
adapter.setup();

const stableAlias = adapter;
const stable = stableAlias.createEntity(5);
stableAlias.addComponent(stable);
out.stable = stableAlias.destroyEntity(stable);

const originalCreate = adapter.createEntity;
adapter.createEntity = function (id) {
  return { id: id + 100, components: 7 };
};
const replaced = adapter.createEntity(2);
out.replacement = replaced.id + replaced.components;
adapter.createEntity = originalCreate;

const originalAdd = adapter.addComponent;
delete adapter.addComponent;
adapter.addComponent = function (entity) {
  entity.components += 4;
};
const recreated = { id: 3, components: 1 };
adapter.addComponent(recreated);
out.deleteRecreate = recreated.components;
adapter.addComponent = originalAdd;

const originalDestroy = adapter.destroyEntity;
Object.defineProperty(adapter, "destroyEntity", {
  configurable: true,
  get() {
    return function (entity) {
      return entity.id + 1000;
    };
  },
});
out.accessor = adapter.destroyEntity({ id: 4, components: 0 });
Object.defineProperty(adapter, "destroyEntity", {
  configurable: true,
  enumerable: true,
  writable: true,
  value: originalDestroy,
});

adapter.tag = 10;
adapter.functionValue = function (value) {
  return this.tag + value;
};
out.functionValue = adapter.functionValue(2);
const rebound = adapter.functionValue;
out.rebound = rebound.call({ tag: 30 }, 2);

const extracted = adapter.createEntity;
out.extracted = extracted.call(adapter, 9).id;

const proxy = new Proxy(adapter, {
  get(target, key) {
    return target[key];
  },
});
out.proxy = proxy.createEntity(12).id;

Object.setPrototypeOf(adapter, {
  createEntity() {
    return { id: -1, components: -1 };
  },
});
out.prototypeMutation = adapter.createEntity(13).id;

let reassigned = adapter;
reassigned = {
  createEntity(id) {
    return { id: id + 500, components: 0 };
  },
};
out.reassignedAlias = reassigned.createEntity(1).id;

console.log(JSON.stringify(out));
