// A write to Array.prototype the compiler cannot attribute (a helper with a
// computed key) must reach direct array method sites: Pristine(Array) is the
// shadow bit of %Array.prototype%, set by the array named-property store.
// DEC 250 (intrinsic-only): Pristine(Array) is the shadow bit of
// %Array.prototype% itself. The patch is refused
// now (expected file); the reads after it still run.
function put(o: any, k: string, v: any) { o[k] = v; }
const arr: number[] = [3, 1, 2];
console.log(arr.join("-"), arr.indexOf(1));
try { put(Array.prototype, process.argv[2] || "join", function (this: any) { return "J" + this.length; }); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
try { put(Array.prototype, process.argv[3] || "indexOf", function () { return -7; }); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
console.log(arr.join("-"), arr.indexOf(1), [5].join());
