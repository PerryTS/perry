// Collection sites: `getIt`'s parameter is annotated a Map and holds an Array.
// The facts give the site no Map receiver (its receiver holds an object of
// another birth), so the folded `get` tests the receiver's brand and takes the
// ordinary method call (TypeError: m.get is not a function). Under
// PERRY_FACTS_SABOTAGE=collsite every site is claimed a Map and a Set
// receiver: the site keeps its exact lowering, and the check-mode test in
// front of it must report that the receiver is no Map ("method site").
// (Recursive, so that the call is not inlined.)
function getIt(m: Map<number, string>, depth: number): any {
  if (depth > 0) return getIt(m, depth - 1);
  return m.get(1);
}
const real = new Map<number, string>();
real.set(1, "one");
console.log(getIt(real, 2));
try {
  console.log(getIt([7, 8] as any, 2));
} catch (e: any) {
  console.log(e.name);
}
