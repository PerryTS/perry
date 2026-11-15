// Named lengths: `walk` reads `xs`, which may hold the Array of `own`, and
// pops `own` by that name in a loop without calls. The pop's receiver is
// not read from `xs`, so the facts give `xs` no named length and the loop
// reads `xs.length` at every step. Under PERRY_FACTS_SABOTAGE=namedlen
// every Array local is claimed to have one: the loop's walker would take
// the pop for a store into another Array, keep the length 6 and read three
// elements past the end. The check-mode witness in the loop's condition
// block must report the changed length ("named length").
function walk(k: number): string {
  const own = [1, 2, 3, 4, 5, 6];
  const other = [7, 8, 9];
  const xs = k ? own : other;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    own.pop();
  }
  return s;
}
console.log(walk(1), walk(0));
