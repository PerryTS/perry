// A Map or Set has no indexed own properties: a keyed read `m[k]` with a
// key computed at run time is the ordinary [[Get]] of the property key, so
// own expandos and then the prototype chain answer it, never the storage.
const map: any = new Map([["x", 1], ["y", 2]]);
map.marker = 71;
const set: any = new Set(["a", "b"]);
set.tag = "own";

function read(o: any, k: any): any { return o[k]; }

const keys = ["marker", "constructor", "get", "size", "has", "missing"];
console.log("map", keys.map((k) => typeof read(map, k)).join(" "));
console.log("map values", read(map, "marker"), read(map, "size"), read(map, "constructor") === Map);
console.log("map index", read(map, 0), read(map, "0"), read(map, 1));
console.log("set", read(set, "tag"), typeof read(set, "add"), read(set, "size"), read(set, 0));

// ToPropertyKey runs on an object key.
const objKey = { toString() { return "marker"; } };
console.log("object key", read(map, objKey));

// A symbol key reads the prototype chain too.
console.log("symbol", typeof read(map, Symbol.iterator), typeof read(set, Symbol.iterator));

// A subclass prototype member is found through the chain.
class Tagged extends Map<string, number> {
  label() { return "tagged"; }
  get kind() { return "tagged-map"; }
}
const t: any = new Tagged([["k", 1]]);
console.log("subclass", typeof read(t, "label"), read(t, "kind"), read(t, "label").call(t));

// Object.prototype members inherited through %Map.prototype%.
console.log("inherited", typeof read(map, "hasOwnProperty"), read(map, "hasOwnProperty").call(map, "marker"));
