// Closed World: a second declarator write disqualifies the proof.
function probe(): string {
  const out: string[] = [];
  out.push(V ? "truthy" : "falsy");
  out.push(typeof V);
  out.push(String(Array.isArray(V)));
  try { out.push(String(V.length)); } catch (e) { out.push((e as Error).constructor.name); }
  try { out.push(String(V[1])); } catch (e) { out.push((e as Error).constructor.name); }
  try { out.push(String(V.indexOf(20))); } catch (e) { out.push((e as Error).constructor.name); }
  try { out.push(V.join("-")); } catch (e) { out.push((e as Error).constructor.name); }
  return out.join(",");
}
var V: any = [10, 20, 30];
console.log("first", probe());
var V: any = "a20b";
console.log("second", probe());
