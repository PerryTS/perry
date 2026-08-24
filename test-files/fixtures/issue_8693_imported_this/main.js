import { Registry } from './registry.js';

const registry = new Registry();
const iterations = 200_000;
const start = performance.now();
for (let i = 0; i < iterations; i++) {
  const entity = { id: i };
  registry.add(entity);
  registry.remove(entity);
}
console.log(JSON.stringify({
  elapsedMs: performance.now() - start,
  remaining: registry.group.entities.length,
}));
