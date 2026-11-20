// Relinking a kind prototype changes the inherited metadata accessors and
// `constructor` of every typed array of that kind.
function read(o: any): string {
  const c = o.constructor;
  return [o.length, o.byteLength, o.BYTES_PER_ELEMENT, typeof c === "function" ? c.name : String(c)].join(" ");
}
const u = new Uint32Array(5);
const d = new Float64Array(2);
console.log(read(u), "|", read(d));
Object.setPrototypeOf(Uint32Array.prototype, { length: "own-length", constructor: "x" });
console.log(read(u), "|", read(d));
