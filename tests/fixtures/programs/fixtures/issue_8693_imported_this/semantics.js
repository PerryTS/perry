import {
  Registry as ImportedRegistry,
  TowerRegistry,
} from './barrel.js';

function entity(id) {
  return { id };
}

const results = [];
const stable = new ImportedRegistry();
const first = entity(1);
stable.add(first);
stable.remove(first);
results.push(`stable:${stable.group.entities.length}`);

class RegistryAdapter {
  constructor(registry) {
    this.registry = registry;
  }

  cycle(value) {
    this.registry.cycle(value);
  }
}

const adapter = new RegistryAdapter(new TowerRegistry());
adapter.cycle(entity(101));
results.push(`adapter:${adapter.registry.left.length}:${adapter.registry.right.length}`);

const shadowed = new ImportedRegistry();
shadowed.add = function (value) {
  this.group.entities.push(entity(value.id + 100));
};
shadowed.add(entity(2));
results.push(`shadow:${shadowed.group.entities[0].id}`);
delete shadowed.add;
shadowed.remove(shadowed.group.entities[0]);
results.push(`unshadow:${shadowed.group.entities.length}`);

const extracted = stable.add;
let extractedThrows = false;
try {
  extracted(entity(3));
} catch (_error) {
  extractedThrows = true;
}
const called = entity(4);
extracted.call(stable, called);
extracted.apply(stable, [entity(5)]);
const rebound = extracted.bind(stable);
rebound(entity(6));
results.push(`binding:${extractedThrows}:${stable.group.entities.length}`);
stable.group.entities.length = 0;

class Holder {
  constructor(registry) {
    this._registry = registry;
  }

  get registry() {
    return this._registry;
  }
}

const holder = new Holder(stable);
const accessed = entity(7);
holder.registry.add(accessed);
holder.registry.remove(accessed);
const proxy = new Proxy(stable, {
  get(target, key, receiver) {
    return Reflect.get(target, key, receiver);
  },
});
proxy.add(entity(8));
results.push(`accessor-proxy:${stable.group.entities[0].id}`);
stable.group.entities.length = 0;

class DerivedRegistry extends ImportedRegistry {
  add(value) {
    super.add(value);
    this.added = (this.added || 0) + 1;
  }
}

const derived = new DerivedRegistry();
derived.add(entity(9));
derived.remove(derived.group.entities[0]);
results.push(`inheritance:${derived.added}:${derived.group.entities.length}`);

const mutation = new ImportedRegistry();
const originalAdd = ImportedRegistry.prototype.add;
ImportedRegistry.prototype.add = function (value) {
  this.group.entities.push(entity(value.id + 1000));
};
mutation.add(entity(10));
delete ImportedRegistry.prototype.add;
let deletedThrows = false;
try {
  mutation.add(entity(11));
} catch (_error) {
  deletedThrows = true;
}
Object.defineProperty(ImportedRegistry.prototype, 'add', {
  value: originalAdd,
  writable: true,
  configurable: true,
});
mutation.add(entity(12));
results.push(`prototype:${mutation.group.entities[0].id}:${deletedThrows}:${mutation.group.entities[1].id}`);

class PrivateCounter {
  #count = 0;

  bump(shouldThrow) {
    this.#count++;
    if (shouldThrow) throw new Error('private-boom');
    return this.#count;
  }

  read() {
    return this.#count;
  }
}

const counter = new PrivateCounter();
let exception = '';
try {
  counter.bump(true);
} catch (error) {
  exception = error.message;
}
counter.bump(false);
results.push(`private-exception:${exception}:${counter.read()}`);

for (let i = 0; i < 20_000; i++) {
  const value = entity(i);
  stable.add(value);
  stable.remove(value);
}
results.push(`gc:${stable.group.entities.length}`);

console.log(results.join('|'));
