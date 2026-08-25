import adapter from "./adapter.js";

adapter.setup();
let checksum = 0;
const iterations = 200_000;
for (let i = 0; i < iterations; i++) {
  const entity = adapter.createEntity(i);
  adapter.addComponent(entity);
  adapter.addComponent(entity);
  checksum += adapter.destroyEntity(entity);
}
console.log(JSON.stringify({
  checksum,
  remaining: adapter.store.entities.length,
}));
