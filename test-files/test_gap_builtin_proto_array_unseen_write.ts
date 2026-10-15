// A write to Array.prototype the compiler cannot attribute (a helper with a
// computed key) must reach direct array method sites: Pristine(Array) is the
// shadow bit of %Array.prototype%, set by the array named-property store.
function put(o: any, k: string, v: any) { o[k] = v; }
const arr: number[] = [3, 1, 2];
console.log(arr.join("-"), arr.indexOf(1));
put(Array.prototype, process.argv[2] || "join", function (this: any) { return "J" + this.length; });
put(Array.prototype, process.argv[3] || "indexOf", function () { return -7; });
console.log(arr.join("-"), arr.indexOf(1), [5].join());
