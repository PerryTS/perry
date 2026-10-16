// Closed World: an ES-module export reaching a reader can be called by a
// module in an import cycle before this module's body assigns the var.
import { early } from "./cwvar/esm_cycle_b.ts";

export function readV(): string {
  const out: string[] = [];
  out.push(V ? "truthy" : "falsy");
  out.push(typeof V);
  try { out.push(String(V.length)); } catch (e) { out.push((e as Error).constructor.name); }
  try { out.push(V.join("-")); } catch (e) { out.push((e as Error).constructor.name); }
  return out.join(",");
}

console.log("early", early);
var V: any = (function () {
  var a: any = [];
  a.push(1);
  a.push(2);
  return a;
})();
console.log("late", readV());
