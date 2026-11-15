// `RegExp.prototype.flags` reads the flag properties through ordinary Get, observable through getters. node reads
// hasIndices, global, ignoreCase, multiline, dotAll, unicode, sticky, unicodeSets (sticky BEFORE unicodeSets) and
// builds the text in d g i m s u v y order. Before, Perry read unicodeSets before sticky.
// Negative controls: the text order with every flag on, a plain object with some flags, a real RegExp, a getter that
// throws (reads stop there), a Symbol receiver (TypeError).
const names = ["hasIndices", "global", "ignoreCase", "multiline", "dotAll", "unicode", "unicodeSets", "sticky"];
const get = Object.getOwnPropertyDescriptor(RegExp.prototype, "flags")!.get!;
function probe(on: string[]): string {
  const log: string[] = [];
  const o: any = {};
  for (const n of names) {
    Object.defineProperty(o, n, { get() { log.push(n); return on.includes(n); } });
  }
  return get.call(o) + " " + log.join(",");
}
console.log(probe(names));
console.log(probe([]));
console.log(probe(["sticky", "unicodeSets", "global"]));
const partial: any = { global: true, sticky: 1, unicodeSets: "" };
console.log(get.call(partial));
console.log(/a/gimsuy.flags, /a/dv.flags, new RegExp("a", "y").flags);
const thrower: any = {};
const seen: string[] = [];
for (const n of names) {
  Object.defineProperty(thrower, n, {
    get() {
      seen.push(n);
      if (n === "sticky") throw new Error("stop");
      return true;
    },
  });
}
try {
  get.call(thrower);
} catch (e: any) {
  console.log("threw", e.message, seen.join(","));
}
try {
  get.call(Symbol("s") as any);
} catch (e: any) {
  console.log("symbol", e.name);
}
