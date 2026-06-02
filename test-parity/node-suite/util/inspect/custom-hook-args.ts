import { inspect } from "node:util";

// #1247: hook receives (depth, options, inspect). depth is the remaining
// budget (default 2 at top level).
const depthHook: any = {
  [inspect.custom](depth: number) {
    return depth === null ? "<no-depth>" : "depth=" + depth;
  },
};
console.log(depthHook);

// String-return hook flows verbatim.
const strHook: any = { [inspect.custom]: () => "<plain-string>" };
console.log(strHook);

// #1251: non-string hook return recurses with the standard depth cap so
// `[Object]` truncation matches Node.
const nestedHook: any = {
  [inspect.custom]: () => ({ x: { y: { z: { w: 1 } } } }),
};
console.log(nestedHook);

// #1250: Object.defineProperty with a symbol key routes into the
// symbol side-table.
const defined: any = { name: "x" };
Object.defineProperty(defined, inspect.custom, {
  value: () => "<hooked-via-defineProperty>",
  enumerable: false,
});
console.log(defined);
