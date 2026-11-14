// `isWhite`'s parameter is passed a literal and a String built at run time
// with the same spelling. Under PERRY_FACTS_SABOTAGE=litstr the facts take
// every binding that may hold a String for one that holds only literals'
// values (`literal_strings::constants`), so `c === "white"` is lowered as
// one compare with the pool's pointer, which answers "unequal" for the
// built String. The witness at the compare runs the complete compare
// beside it and must report the difference.
// A value the compiler cannot fold keeps the argument a run-time String.
const RUN = Math.random() < 2 ? "ite" : "";
function isWhite(c: string): number {
  return c === "white" ? 10 : 1;
}
const values: string[] = ["white", "black", "wh" + RUN];
let s = 0;
for (const v of values) s += isWhite(v);
console.log(s);
