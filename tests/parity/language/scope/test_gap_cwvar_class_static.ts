// Closed World: a class static initializer reaching a reader runs at the
// class definition, before the module-level var is assigned.
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
const log: string[] = [];
// Nothing before the declarator names K: only its static initializer, run at
// the class definition, reaches the reader.
class K {
  static early: string = log.push(probe()) > 0 ? "recorded" : "";
  read(): string {
    return probe();
  }
}
var V: any = (function () {
  var a: any = [];
  for (var i = 1; i <= 3; i++) a.push(i * 10);
  return a;
})();
console.log("static", log[0], K.early);
console.log("late", new K().read());
