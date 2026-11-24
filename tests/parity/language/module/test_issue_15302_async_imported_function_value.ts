// An imported function an async body uses as a value is the same function value a
// synchronous body and its own module see: callable with numbers through a variable
// or an argument, an async function included.
import { add, inc, dbl, getAdd, isAdd } from "./fixtures/async_extern_funcref/lib.ts";

function twice(f: (x: number) => number, x: number) { return f(f(x)); }
const syncAdd = add;
console.log("sync", syncAdd(1, 2), twice(inc, 5));

async function run(): Promise<string> {
  const g = add;
  const d = dbl;
  return [g(1, 2), twice(inc, 5), typeof add, await d(4), g === syncAdd, g === getAdd(), isAdd(g)].join(",");
}
run().then((v) => console.log("async", v), (e) => console.log("threw " + e.message));
