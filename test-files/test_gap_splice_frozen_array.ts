// splice on a frozen array throws a TypeError and leaves the array intact,
// whatever the arguments: deletes, inserts, a pure no-op, direct or reflective.
const splice: any = Array.prototype.splice;
const cases: any[][] = [[0, 1], [0, 0, 9], [1, 1, 8], [0, 0], [5], []];
for (const args of cases) {
  const a = Object.freeze([1, 2, 3]);
  try { (a as any).splice(...args); console.log(JSON.stringify(args), "no throw"); }
  catch (e) { console.log(JSON.stringify(args), e instanceof TypeError, a.join(",")); }
  try { Reflect.apply(splice, a, args); console.log("reflect no throw"); }
  catch (e) { console.log("reflect", e instanceof TypeError, a.join(",")); }
}
const s = Object.seal([1, 2, 3]);
s.splice(0, 1, 7);
console.log("sealed replace", s.join(","));
