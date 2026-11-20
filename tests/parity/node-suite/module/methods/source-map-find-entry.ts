import mod from "node:module";

// #3675: SourceMap.findEntry / findOrigin must return real mapping objects.

// Names-less single mapping (the issue repro).
const map = new (mod as any).SourceMap({
  version: 3,
  file: "out.js",
  sources: ["input.ts"],
  names: [],
  mappings: "AAAA",
});
console.log("entry(0,0):", JSON.stringify(map.findEntry(0, 0)));
console.log("entry(5,5):", JSON.stringify(map.findEntry(5, 5)));
console.log("origin(0,0):", JSON.stringify(map.findOrigin(0, 0)));
console.log("origin(str,0,0):", JSON.stringify(map.findOrigin("input.ts", 0, 0)));

// Explicit 5-field (named) segment.
const named = new (mod as any).SourceMap({
  version: 3,
  file: "o.js",
  sources: ["a.ts"],
  names: ["fn"],
  mappings: "AAAAA",
});
console.log("named entry:", JSON.stringify(named.findEntry(0, 0)));
const originArgs: any[][] = [
  [0, 0],
  ["a.ts", 0, 0],
  ["a.ts", 5],
  [1, 2],
  ["x"],
  [],
  [2, "a.ts"],
];
for (const args of originArgs) {
  console.log("origin", JSON.stringify(args), "=>", JSON.stringify(named.findOrigin(...args)));
}

// Multi-source mapping with cumulative source/line/column deltas.
const multi = new (mod as any).SourceMap({
  version: 3,
  file: "o.js",
  sources: ["a.ts", "b.ts"],
  names: [],
  mappings: "AAAA,CAAC;ACDA",
});
console.log("multi(0,1):", JSON.stringify(multi.findEntry(0, 1)));
console.log("multi(1,0):", JSON.stringify(multi.findEntry(1, 0)));
