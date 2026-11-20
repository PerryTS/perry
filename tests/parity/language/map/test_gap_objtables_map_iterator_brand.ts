import { types } from "node:util";
const map = new Map<number, number>([[1, 11], [2, 22]]);
const iterator = map.entries();
for (let i = 0; i < 30000; i++) { const pressure = {i, values: [i, i + 1]}; if (pressure.i < 0) console.log(pressure); }
console.log(types.isMapIterator(iterator), types.isSetIterator(iterator), types.isMapIterator([]));
console.log(iterator.next().value.join(":"));
map.set(3, 33);
console.log(iterator.next().value.join(":"));
console.log(iterator.next().value.join(":"));
console.log(iterator.next().done);
