// A Map or Set has no indexed own properties: a keyed store `m[k] = v` with a
// key computed at run time is the ordinary [[Set]] of the property key. It
// creates an own property (or runs an inherited setter), never a storage write.
const map: any = new Map([["x", 1]]);
const set: any = new Set(["a"]);

function write(o: any, k: any, v: any): any { o[k] = v; return o; }
function read(o: any, k: any): any { return o[k]; }

write(map, 0, "zero");
write(map, 1.5, "frac");
write(map, "name", "n");
write(set, 0, "s0");
write(set, 7, 70);
console.log("map own", JSON.stringify(Object.keys(map)), read(map, 0), read(map, "0"), read(map, 1.5), map.size, map.get(0));
console.log("set own", JSON.stringify(Object.keys(set)), read(set, 0), set[7], set.size, set.has("s0"));
console.log("hasOwn", Object.prototype.hasOwnProperty.call(map, "0"), Object.prototype.hasOwnProperty.call(set, "7"));

// An inherited setter on a subclass prototype runs with the receiver.
const seen: string[] = [];
class Logged extends Map<string, number> {
  set 3(v: any) { seen.push("setter " + v + " " + (this instanceof Logged)); }
}
const lg: any = new Logged();
write(lg, 3, "three");
console.log("setter", seen.join(","), Object.keys(lg).length, lg.size);

// A setter on %Set.prototype% for an index key.
Object.defineProperty(Set.prototype, "5", {
  set(v: any) { seen.push("proto5 " + v + " " + (this === set)); },
  configurable: true,
});
write(set, 5, "five");
console.log("proto setter", seen[seen.length - 1], Object.prototype.hasOwnProperty.call(set, "5"));
delete (Set.prototype as any)["5"];

// A frozen Map rejects the store; strict code throws.
const frozen: any = Object.freeze(new Map());
try { write(frozen, 2, "x"); console.log("frozen", "no throw", frozen[2]); }
catch (e: any) { console.log("frozen", e instanceof TypeError, frozen[2]); }

// An object key runs ToPropertyKey once.
let calls = 0;
const objKey = { toString() { calls++; return "viaKey"; } };
write(map, objKey, 9);
console.log("object key", map.viaKey, calls);
