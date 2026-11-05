const keys: any[] = [];
const set = new Set<any>();
for (let i = 0; i < 12; i++) {
  const key: any = i % 2 === 0 ? { i } : [i];
  keys.push(key);
  set.add(key);
}
for (let i = 0; i < 30000; i++) {
  const pressure = { i, data: [i, i + 1] };
  if (pressure.i < 0) console.log(pressure);
}
let found = 0;
for (const key of keys) if (set.has(key)) found++;
console.log(set.size, found, set.has({ i: 0 }), set.has([1]));
const array = keys[1];
array.push(13, 14, 15, 16, 17, 18, 19, 20);
console.log(set.has(array), set.delete(keys[0]), set.has(keys[0]), set.size);
set.add(keys[0]);
console.log(set.has(keys[0]), set.size);
