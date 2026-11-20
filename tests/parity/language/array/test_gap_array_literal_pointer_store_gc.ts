// An array literal is born pointer-free; storing a heap value into it in
// place must take it out of that state, or the next collection skips the
// slot and the element is a stale pointer. The p-queue shape in Next.js's
// bundled runtimes hit it through a class capturing a reassigned `let`
// (`this._resolveEmpty = empty` then `this._resolveEmpty()` threw
// "_resolveEmpty is not a function" after a scavenge).
function churn(n: number): void {
  let g: object[] = [];
  for (let k = 0; k < n; k++) {
    g.push({ k, s: "x" + k });
    if (g.length > 50000) g = [];
  }
}

const fns: any[] = [undefined];
fns[0] = () => 1;
const objs: any[] = [0, 0];
objs[1] = { x: 7 };
const strs: any[] = [true];
strs[0] = "s-" + Math.random().toString(36).slice(2, 3).length;

const out: any = {};
(() => {
  "use strict";
  let t: any, s: any;
  t = class Base {};
  s = () => "empty";
  out.Q = class extends t {
    _resolveEmpty: any;
    constructor() {
      super();
      this._resolveEmpty = s;
    }
    run() {
      return this._resolveEmpty();
    }
  };
  out.get = () => s;
})();

churn(2000000);

console.log(typeof fns[0], fns[0]());
console.log(typeof objs[1], objs[1].x);
console.log(strs[0]);
console.log(typeof out.get(), out.get()());
console.log(new out.Q().run());
