// Quiet sites over Strings: `walk` keeps no length over its loop, because
// the operator `n + vs[k]` may run program code: `vs` holds `V`s, whose
// `valueOf` pops the Array the loop walks. The element site `vs[k]` is
// quiet (a plain Array under a Number key), but its value is an object, so
// the String rule (a quiet element read over Strings alone gives a
// primitive) does not apply and the walker reads `xs.length` at every step.
// Under PERRY_FACTS_SABOTAGE=quiet-strings every quiet element read is
// claimed to give a primitive: the walker keeps the length 6 and the loop
// reads three elements past the end. The check-mode witness in the loop's
// condition block must report the changed length ("quiet sites").
const H = { xs: [1, 2, 3, 4, 5, 6] };
class V {
  valueOf(): number {
    H.xs.pop();
    return 1;
  }
}
function walk(vs: V[]): string {
  const xs = H.xs;
  let s = "";
  let n = 0;
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    n = n + vs[0];
  }
  return s + n;
}
console.log(walk([new V()]));
