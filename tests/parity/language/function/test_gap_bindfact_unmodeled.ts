// PLAN 3h: Function.prototype reached through a value the whole-program
// analysis cannot follow (a computed global) and written there.
function f() { return 1; }
const name = ["Func", "tion"].join("");
const F: any = (globalThis as any)[name];
const original = F.prototype.bind;
const out: string[] = [];
for (let i = 0; i < 4; i++) {
  try { if (i === 2) F.prototype.bind = function () { return "unmodeled"; }; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
  const b = f.bind(null);
  out.push([i, typeof b === "function" ? "builtin" : b].join(" "));
}
try { F.prototype.bind = original; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
for (const l of out) console.log(l);
