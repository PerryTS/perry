// PLAN 3h + decision 150: compiled code never writes Function.prototype
// (the closed-world fact holds); interpreted code from a run-time string
// replaces bind mid-run, which the sites' interpreted-write test must see.
class C {
  constructor() { this.m = this.m.bind(this); }
  m(): string { return "m"; }
}
interface C { m: any; }
function f() { return 1; }
const original = Function.prototype.bind;
const src = ["Function.prototype.", "bind = function () { return 'from eval'; }"].join("");
const out: string[] = [];
for (let i = 0; i < 4; i++) {
  if (i === 2) (0, eval)(src);
  const b = f.bind(null), m = new C().m;
  out.push([i, typeof b === "function" ? "builtin" : b, typeof m === "function" ? "builtin" : m].join(" "));
}
const restore = ["Function.prototype.bind = ", "globalThis.__bindfact_original"].join("");
(globalThis as any).__bindfact_original = original;
(0, eval)(restore);
for (const l of out) console.log(l);
