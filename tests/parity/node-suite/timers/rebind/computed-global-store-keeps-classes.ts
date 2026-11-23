// A computed-key store into globalThis may rebind any global, but program
// declarations (classes, functions, let/const bindings) stay program bindings.
class Foo {
  v = 7;
}
function helper() {
  return "helper";
}
let counter = 3;
const n = 1;
console.log(typeof Foo, n);
const g = globalThis as any;
g[String(process.argv.length)] = 0;
console.log(typeof Foo, typeof helper, typeof counter, n);
const f = new Foo();
console.log(f.v, helper(), counter + 1, f instanceof Foo);
