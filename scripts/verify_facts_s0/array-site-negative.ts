// Method sites: `find`'s parameter is annotated an Array and holds an Array
// with its own `indexOf`. The facts give the site no Array-member verdict
// (a receiver holds the key as an own property), so the folded
// `Expr::ArrayIndexOf` keeps its run-time guard and runs the own method.
// Under PERRY_FACTS_SABOTAGE=arraysite every site is claimed a member site:
// the node drops its guard, and the check-mode witness in front of the fold
// (the dropped test) must report the receiver ("method site").
// (Recursive, so that the call is not inlined into the module body.)
function find(a: number[], depth: number): any {
  if (depth > 0) return find(a, depth - 1);
  return a.indexOf(2);
}
const plain = [1, 2, 3];
console.log(find(plain, 2));
const own: any = [1, 2, 3];
own.indexOf = (): string => "own indexOf";
console.log(find(own, 2));
