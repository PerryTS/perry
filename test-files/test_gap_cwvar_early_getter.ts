// Closed World: getters, valueOf and toString run by init code before the
// module-level var is assigned see undefined.
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
const o: any = { get x() { return probe(); } };
console.log("getter", o.x);
const w: any = { valueOf() { return probe().length; }, toString() { return "s:" + probe(); } };
console.log("valueOf", +w);
console.log(`toString ${w}`);
var V: any = (function () {
  var a: any = [];
  for (var i = 1; i <= 3; i++) a.push(i * 10);
  return a;
})();
console.log("late", o.x, +w);
