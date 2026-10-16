// Closed World: a reader reached through a callback during init, before the
// module-level var is assigned, sees undefined.
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
[0].forEach(function () {
  console.log("callback", probe());
});
var V: any = (function () {
  var a: any = [];
  for (var i = 1; i <= 3; i++) a.push(i * 10);
  return a;
})();
[0].forEach(() => console.log("late", probe()));
