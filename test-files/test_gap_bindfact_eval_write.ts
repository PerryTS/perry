// PLAN 3h + decision 150: interpreted code that replaces
// Function.prototype.bind mid-run; the compiled bind sites must see it.
class C {
  constructor() { this.m = this.m.bind(this); }
  m(): string { return "m"; }
}
interface C { m: any; }
function f() { return 1; }
const original = Function.prototype.bind;
const src = "Function.prototype.bind = function () { return 'from eval'; }";
const out: string[] = [];
for (let i = 0; i < 4; i++) {
  if (i === 2) (0, eval)(src);
  const b = f.bind(null), m = new C().m;
  out.push([i, typeof b === "function" ? "builtin" : b, typeof m === "function" ? "builtin" : m].join(" "));
}
Function.prototype.bind = original;
for (const l of out) console.log(l);
