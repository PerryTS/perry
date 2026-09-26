// Map, Set, JSON, spread and destructuring.
const m = new Map<string, number>([["a", 1], ["b", 2]]);
m.set("c", 3);
console.log(m.size, m.get("b"), [...m.keys()].join(""));
const set = new Set([1, 2, 2, 3]);
console.log(set.size, set.has(2), [...set]);
const o = JSON.parse('{"x":[1,2,{"y":"z"}],"n":null}');
console.log(o.x[2].y, JSON.stringify(o), JSON.stringify({ a: [1, 2] }, null, 1));
const { x, ...rest } = { x: 1, y: 2, z: 3 };
console.log(x, rest);
