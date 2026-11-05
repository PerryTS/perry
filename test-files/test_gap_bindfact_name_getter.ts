// PLAN 3h: an accessor on Function.prototype's `name` (bind reads its
// target's own `name`, so it does not run), and a deleted
// Function.prototype.bind, which leaves functions without `bind`.
class C {
  tag = "c";
  constructor() { this.m = this.m.bind(this); this.after = 1; }
  m(): string { return this.tag; }
}
interface C { m: any; after: number; }
function f() { return 1; }
const out: string[] = [];
out.push(["before", f.bind(null).name, new C().m.name].join(" "));
try { Object.defineProperty(Function.prototype, "name", { get() { return "G"; }, configurable: true }); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
const c = new C();
out.push(["getter", f.bind(null).name, c.m.name, c.m(), c.after].join(" "));
const original = Function.prototype.bind;
try { delete (Function.prototype as any).bind; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
try { (f as any).bind(null); out.push("no throw"); }
catch (e: any) { out.push("deleted bind throws " + (e instanceof TypeError)); }
try { new C(); out.push("no throw"); }
catch (e: any) { out.push("ctor bind throws " + (e instanceof TypeError)); }
try { Function.prototype.bind = original; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
for (const l of out) console.log(l);
