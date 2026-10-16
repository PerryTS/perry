// A Next.js-shaped program (a module under `.next/server/`) gets
// `globalThis.AsyncLocalStorage` seeded before any module init. The seeded
// constructor must be the real `async_hooks` class: its static `snapshot()` and
// `bind()` capture the current store (perry threw "value is not a function"
// for `snapshot`, and `bind` fell through to Function.prototype.bind).
import "./nextjs_als_seed/baseline.cjs";
import * as page from "./nextjs_als_seed/.next/server/page.cjs";

console.log(page.staticTypes().join(" "));
const als = page.create();
let snap: any;
let bound: any;
als.run(7, () => {
  snap = page.createSnapshot();
  bound = page.bindSnapshot((x: number) => [x, als.getStore()]);
});
console.log("outside", als.getStore());
console.log("snapshot", snap(() => als.getStore()));
console.log("snapshot args", snap((a: number, b: number) => a + b + als.getStore(), 1, 2));
console.log("bound", JSON.stringify(bound(5)));
als.run(8, () => console.log("snapshot inside other run", snap(() => als.getStore())));
