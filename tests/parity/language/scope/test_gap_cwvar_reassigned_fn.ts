// Closed World: a module-level var reassigned inside a function is not a
// constant of its initializer.
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
var V: any = (function () {
  var a: any = [];
  for (var i = 1; i <= 3; i++) a.push(i * 10);
  return a;
})();
function reset(): void {
  V = "x20y";
}
console.log("before", probe());
reset();
console.log("after", probe());
