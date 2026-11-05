import { types } from "node:util";
const set = new Set<number>([11, 22]);
const iterator = set.values();
for (let i = 0; i < 30000; i++) { const pressure = {i, values: [i, i + 1]}; if (pressure.i < 0) console.log(pressure); }
console.log(types.isSetIterator(iterator), types.isMapIterator(iterator), types.isSetIterator([]));
console.log(iterator.next().value);
set.add(33);
console.log(iterator.next().value);
console.log(iterator.next().value);
console.log(iterator.next().done);
