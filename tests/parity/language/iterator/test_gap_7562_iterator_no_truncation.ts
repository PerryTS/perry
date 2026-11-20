// #7562: spread / Array.from must not silently truncate a long iterator.
// The drain loops carried a hardcoded 100,000-element "safety limit" and
// returned a SHORT array when it was hit — no error, no warning, a
// plausible-looking wrong answer. Compared byte-for-byte against node.
const m = new Map<string, number>();
for (let i = 0; i < 150000; i++) m.set("k" + i, i);
console.log("map size:", m.size);
console.log("spread values:", [...m.values()].length);
console.log("spread keys:", [...m.keys()].length);
console.log("Array.from values:", Array.from(m.values()).length);
console.log("spread map:", [...m].length);

const s = new Set<number>();
for (let i = 0; i < 130000; i++) s.add(i);
console.log("set size:", s.size);
console.log("spread set:", [...s].length);

// A generator past the old bound.
function* gen(n: number) { for (let i = 0; i < n; i++) yield i; }
const g = [...gen(120000)];
console.log("generator spread:", g.length, "last:", g[g.length - 1]);

// Small iterators must be untouched.
console.log("small:", [...new Map([["a", 1], ["b", 2]]).values()].join(","));
