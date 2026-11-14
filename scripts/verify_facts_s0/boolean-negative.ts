// `pick`'s parameter is declared `boolean` and passed a Number and a String
// beside Booleans. Under PERRY_FACTS_SABOTAGE=bools the facts take every
// binding that may hold a Boolean for one that holds nothing else
// (`booleans::holds`): its truthiness is lowered as one compare with
// `true`, which answers "falsy" for `1` and `"x"`. The witness at the
// test must report the word that is neither `true` nor `false`.
// A value the compiler cannot fold keeps the argument a run-time value.
const RUN = Math.random() < 2 ? 1 : 0;
function pick(flag: boolean): number {
  return flag ? 10 : 1;
}
const values: any[] = [true, false, RUN, RUN === 1 ? "x" : ""];
let s = 0;
for (const v of values) s += pick(v);
console.log(s);
