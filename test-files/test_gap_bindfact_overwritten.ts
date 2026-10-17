// PLAN 3h: a program that replaces Function.prototype.bind has no
// "bind unwritten" fact; its bind sites call the replacement. (Output is
// collected and printed after bind is restored: console uses bind.)
class C {
  constructor() { this.m = this.m.bind(this); }
  m(): string { return "m"; }
}
interface C { m: any; }
function f() { return 1; }
const out: string[] = [];
out.push(["before", typeof f.bind(null), typeof new C().m].join(" "));
const original = Function.prototype.bind;
(Function.prototype as any).bind = function (this: any, ...a: any[]) { return "patched:" + a.length; };
out.push(["after", f.bind(null), (f as any).bind(null, 1, 2), new C().m].join(" "));
Function.prototype.bind = original;
out.push(["restored", typeof f.bind(null), typeof new C().m].join(" "));
for (const l of out) console.log(l);
