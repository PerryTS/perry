function mk() { return function () {}; }
const F: any = mk();
Object.defineProperty(F, "prototype", { value: undefined });
console.log(F.prototype === undefined);
try {
  class C extends F {}
  console.log(false);
} catch (e) {
  console.log(e instanceof TypeError);
}

const B: any = mk().bind(null);
Object.defineProperty(B, "prototype", { value: {}, configurable: true });
let reads = 0;
Object.defineProperty(B, "prototype", { get() { reads++; return undefined; } });
try {
  class D extends B {}
  console.log(false);
} catch (e) {
  console.log(e instanceof TypeError, reads);
}
