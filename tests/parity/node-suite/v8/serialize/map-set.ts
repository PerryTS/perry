import { deserialize, serialize } from "node:v8";

const key = { key: true };
const source = {
  map: new Map<any, any>([["a", 1], [key, key]]),
  set: new Set<any>([3, "two", key]),
};
const result: any = deserialize(serialize(source));
const clonedKey = [...result.map.keys()][1];

console.log(
  "map:",
  result.map instanceof Map,
  result.map.size,
  JSON.stringify([...result.map]),
);
console.log(
  "set:",
  result.set instanceof Set,
  result.set.size,
  JSON.stringify([...result.set]),
);
console.log("map identity:", result.map.get(clonedKey) === clonedKey);
console.log("shared identity:", result.set.has(clonedKey));
