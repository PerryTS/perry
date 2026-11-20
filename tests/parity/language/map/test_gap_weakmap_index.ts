// #10057: growth, identity, overwrite, deletion and free-slot reuse.
const keys: object[] = [];
const map = new WeakMap<object, number>();
const set = new WeakSet<object>();
for (let i = 0; i < 4096; i++) {
  const key = { same: true };
  keys.push(key);
  map.set(key, i);
  set.add(key);
}
let sum = 0;
for (let i = 0; i < keys.length; i++) {
  sum += map.get(keys[i])!;
  if (!set.has(keys[i])) throw new Error("missing WeakSet member");
}
console.log(sum);
console.log(map.set(keys[0], 99) === map, map.get(keys[0]), map.get(keys[1]));
console.log(set.add(keys[0]) === set);
console.log(map.has({ same: true }), map.get({ same: true }));
for (let i = 0; i < keys.length; i += 2) {
  if (!map.delete(keys[i]) || !set.delete(keys[i])) throw new Error("delete failed");
  if (map.has(keys[i]) || set.has(keys[i])) throw new Error("deleted key found");
  if (map.get(keys[i]) !== undefined || map.delete(keys[i])) throw new Error("stale entry");
}
for (let i = 0; i < keys.length; i += 2) {
  const fresh = { same: true };
  map.set(fresh, i * 2);
  set.add(fresh);
  if (map.has(keys[i]) || set.has(keys[i])) throw new Error("reused slot aliases old key");
  keys[i] = fresh;
}
sum = 0;
for (let i = 0; i < keys.length; i++) {
  sum += map.get(keys[i])!;
  if (!set.has(keys[i])) throw new Error("missing reused member");
}
console.log(sum);
const first = Symbol("same");
const second = Symbol("same");
const symbols = new WeakMap<symbol, number>();
symbols.set(first, 1);
symbols.set(second, 2);
console.log(symbols.get(first), symbols.get(second), symbols.delete(first), symbols.has(second));
