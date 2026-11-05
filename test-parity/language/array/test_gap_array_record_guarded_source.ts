// The private Any payload is still an entry-proven ordinary array. Its reads
// share the existing array backend and retain live descriptor/prototype Get.
function sum(a: any) {
  let n = 0;
  for (const v of a) n += v === undefined ? 0 : v;
  return n;
}
console.log(sum([1, 2, 3]), sum(new Uint8Array([4, 5])), sum(new Set([6, 7])));
const a: any = [1, 2, 3];
let reads = 0;
Object.defineProperty(a, "1", {get() { reads++; return 20; }, configurable: true});
console.log(sum(a), reads);
const b: any = [1, 2];
let result = 0;
for (const v of b) {
  result += v;
  if (v === 1) {
    delete b[1];
    const proto: any = Object.create(Array.prototype);
    Object.defineProperty(proto, "1", {get() { return 30; }});
    Object.setPrototypeOf(b, proto);
    for (let i=3; i<40; i++) b.push(i);
  }
}
console.log(result, b.length);
