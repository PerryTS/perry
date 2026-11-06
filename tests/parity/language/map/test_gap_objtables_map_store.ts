// The entry store and all three key indexes follow an evacuated Map owner.
const objects: object[] = [];
const map = new Map<any, any>();
for (let i = 0; i < 64; i++) {
  const key = {i}; objects.push(key);
  map.set(key, {value: i + 37});
  map.set(`key-${i}`, i * 2);
  map.set(i, i * 3);
}
for (let round = 0; round < 40000; round++) {
  const waste = {round, text: `waste-${round}`, values: [round, round + 1]};
  if (waste.values[0] !== round) throw new Error('allocation');
}
let sum = 0;
for (let i = 0; i < 64; i++) {
  if (map.get(objects[i]).value !== i + 37) throw new Error('object key');
  if (map.get(`key-${i}`) !== i * 2 || map.get(i) !== i * 3) throw new Error('key index');
  sum += map.get(objects[i]).value;
}
const fresh = new Map();
if (fresh.has(objects[0]) || fresh.size !== 0) throw new Error('fresh owner');
for (let i = 0; i < 40; i++) map.delete(objects[i]);
map.set(objects[0], {value: 99});
if (map.get(objects[0]).value !== 99 || fresh.has(objects[0])) throw new Error('owned mutation');
console.log('map-store', sum, map.size, fresh.size);
