import {
  Foreign,
  installIdAccessor,
  makeProxy,
  reshape,
  reshapeAliasedId,
} from "./barrel.ts";

class Entity {
  id: number;
  components: number[];

  constructor(id: number) {
    this.id = id;
    this.components = [];
  }
}

class SubEntity extends Entity {}

let accessorHits = 0;
class AccessorEntity {
  get id(): number {
    accessorHits++;
    return 7;
  }

  get components(): number[] {
    accessorHits++;
    return [4, 5];
  }
}

class Registry {
  read(entity: Entity): number {
    return entity.id + entity.components.length;
  }

  throws(entity: Entity): number {
    const id = entity.id;
    throw "boom-" + id;
  }

  // Bare alias/reassignment uses must keep these methods generic.
  alias(entity: Entity): number {
    const alias = entity;
    return alias.id;
  }

  reassign(entity: Entity): number {
    entity = new Entity(99);
    return entity.id;
  }
}

class ForeignReader {
  // Imported class metadata is intentionally not a local clone capability.
  read(entity: Foreign): number {
    return entity.id + entity.components.length;
  }
}

class AliasReader {
  id: number;
  components: number[];

  constructor(id: number) {
    this.id = id;
    this.components = [1];
  }

  read(other: AliasReader): number {
    reshapeAliasedId(this);
    return other.id + other.components.length;
  }
}

const registry = new Registry();
const results: any[] = [];

const exact = new Entity(2);
exact.components.push(1, 2);
results.push(registry.read(exact));

// Wrong class and multiple caller layouts exercise the explicit generic arm.
results.push(registry.read(({ id: 5, components: [1] } as any) as Entity));
const subclass = new SubEntity(3);
subclass.components.push(8);
results.push(registry.read(subclass));

const changed = new Entity(4);
changed.components.push(1);
(changed as any).extra = true;
results.push(registry.read(changed));

results.push(registry.read((new AccessorEntity() as any) as Entity));

const proxyCounter = { hits: 0 };
const proxied = makeProxy(new Entity(6), proxyCounter);
results.push(registry.read((proxied as any) as Entity));

try {
  registry.throws(new Entity(8));
} catch (error) {
  results.push(error);
}

results.push(registry.alias(new Entity(9)));
results.push(registry.reassign(new Entity(10)));
results.push(new ForeignReader().read(new Foreign(11)));

// The selected argument aliases `this`; the imported call changes its shape
// before the declared-field read. This call must stay out of `$pshape_args`.
const aliased = new AliasReader(12);
results.push(aliased.read(aliased));

const reshaped = new Entity(13);
reshape(reshaped);
results.push(registry.read(reshaped));

const descriptor = new Entity(15);
installIdAccessor(descriptor);
results.push(registry.read(descriptor));

// Own-method replacement must stay ahead of every argument clone route.
(registry as any).read = function (entity: any): number {
  return entity.id * 10;
};
results.push(registry.read(new Entity(17)));

console.log(
  JSON.stringify({ results, accessorHits, proxyHits: proxyCounter.hits }),
);
