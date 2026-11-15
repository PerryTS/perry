// Method sites: `fillIt`'s parameter is annotated a Float64Array and holds an
// ordinary object with its own `fill`. The facts give the site no
// typed-array receiver (its receiver holds an object of another birth), so
// the call is the ordinary method call and runs the object's `fill`. Under
// PERRY_FACTS_SABOTAGE=tasite every site is claimed a typed-array receiver:
// the site keeps its typed-array lowering, and the check-mode witness in
// front of it must report that the receiver is no typed array ("method
// site").
// (Recursive, so that the call is not inlined: an inlined copy reads the
// argument's own binding, which carries no typed-array annotation.)
function fillIt(a: Float64Array, depth: number): any {
  if (depth > 0) return fillIt(a, depth - 1);
  return a.fill(1);
}
const lied: any = {
  fill(): string {
    return "own fill";
  },
};
console.log(fillIt(lied, 2));
const real = new Float64Array(2);
console.log(fillIt(real, 2).join(","));
