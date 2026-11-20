import adapter from "./adapter.js";
import runner from "./runner.js";

adapter.setup();
let checksum = 0;
const iterations = 200_000;
for (let i = 0; i < iterations; i++) {
  checksum += runner.perform(adapter, i);
}
console.log(JSON.stringify({
  checksum,
  remaining: adapter.store.entities.length,
}));
