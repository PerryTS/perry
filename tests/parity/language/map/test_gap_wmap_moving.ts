// Every stress run must also prove loop polls and protected copying minors.
const map: any = new WeakMap();
const set: any = new WeakSet();
const keys: any[] = [];
const get = map.get.bind(map);
const put = map.set.bind(map);
const has = map.has.bind(map);
const member = set.has.bind(set);
for (let i = 0; i < 96; i++) {
  const key = { i };
  keys.push(key);
  put(key, { key, n: i });
  set.add(key);
}
let sum = 0;
for (let round = 0; round < 96; round++) {
  const pressure: any[] = [];
  for (let p = 0; p < 4; p++) pressure.push({ text: "pressure" + round + ":" + p });
  const key = keys[round];
  const value = get(key);
  if (!has(key) || !member(key) || value.key !== key || value.n !== round) throw new Error("moving entry lost");
  sum += value.n + pressure.length;
  put(key, { key, n: round + 100 });
}
for (let i = 0; i < keys.length; i++) {
  if (get(keys[i]).n !== i + 100) throw new Error("overwrite lost");
}
console.log("survived", sum, map.delete(keys[0]), set.delete(keys[0]), has(keys[0]), member(keys[0]));
