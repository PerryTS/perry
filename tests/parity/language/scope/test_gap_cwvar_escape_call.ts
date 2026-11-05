// Closed World: a reader escaping into a call made before the assignment
// (invoked right away, stored and invoked through an object) sees undefined.
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
function callNow(f: () => string): string {
  return f();
}
console.log("callNow", callNow(probe));
const holder: any = {};
holder.f = probe;
console.log("holder", holder.f());
var V: any = (function () {
  var a: any = [];
  for (var i = 1; i <= 3; i++) a.push(i * 10);
  return a;
})();
console.log("late", callNow(probe), holder.f());
