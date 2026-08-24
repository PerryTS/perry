// Reduced wolf-ecs kernel from https://github.com/PerryTS/perry/issues/8692.
// Compile with `PERRY_NO_AUTO_OPTIMIZE=1` for the ticket's stable A/B protocol.
class Query extends Array {
  archetypes = this;
}

class Archetype extends Array {
  entities = this;
}

const entityCount = 1_000;
const iterations = 2_000;
const query = new Query();
const archetype = new Archetype();
for (let i = 0; i < entityCount; i++) archetype.push(i);
query.push(archetype);

const components = new Uint32Array(entityCount);

function system(values) {
  for (let i = 0, length = query.length; i < length; i++) {
    const current = query[i];
    for (let j = 0, length = current.length; j < length; j++) {
      values[current[j]] += 1;
    }
  }
}

const start = performance.now();
for (let i = 0; i < iterations; i++) system(components);
const elapsedMs = performance.now() - start;
console.log(JSON.stringify({ elapsedMs, checksum: components[0] }));
