// Map, Set, JSON
const m = new Map<string, number>();
m.set("a", 1);
m.set("b", 2);
console.log(m.get("a"));
console.log(m.has("b"));
console.log(m.size);

const s = new Set<number>();
s.add(1);
s.add(2);
s.add(2);
console.log(s.size);
console.log(s.has(1));

const obj = { x: 1, y: "two" };
const json = JSON.stringify(obj);
console.log(json);
const parsed = JSON.parse(json);
console.log(parsed.x);
