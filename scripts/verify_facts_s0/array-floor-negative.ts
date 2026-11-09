// `a.pop()` behind a branch the facts cannot decide shrinks `a` to 3. Under
// PERRY_FACTS_SABOTAGE=arrfloor a shrinking event keeps the length interval
// (`solve::arr_len`): the facts claim `a` keeps its 4 elements for good, so
// `a[3]` is a proven element read, and its witness must report the length.
const a = new Array<number>(4).fill(1);
function at(i: number): number {
  return a[i] ?? 7;
}
if (Math.random() < 2) a.pop();
console.log(at(3), a.length);
