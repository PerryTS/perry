// #8774: an exact-shape object passed in an ordinary argument position must
// reach Registry's tagged `$pshape_args` clones.  The hot bodies may access
// Entity's declared fields directly; every other runtime shape stays on the
// generic method entry.
class Entity {
  constructor(id) {
    this.id = id;
    this.components = [];
  }
}

class Registry {
  add(entity, component) {
    entity.components.push(component);
  }

  hash(entity) {
    let value = entity.id;
    for (let i = 0; i < entity.components.length; i++) {
      value += entity.components[i];
    }
    return value;
  }

  clear(entity) {
    entity.components.length = 0;
  }
}

const registry = new Registry();
let checksum = 0;
const iterations = 200_000;
for (let i = 0; i < iterations; i++) {
  const entity = new Entity(i);
  registry.add(entity, 1);
  registry.add(entity, 2);
  checksum += registry.hash(entity);
  registry.clear(entity);
}
console.log(checksum);
