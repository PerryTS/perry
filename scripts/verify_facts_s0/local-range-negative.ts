// The facts' interval of a proven int32 `let` is a range proof in codegen
// (`range_facts::facts_local_range`): `k` is in [3, 18], so `k * 2 + 1` is an
// int32 element key with no guard. Under PERRY_FACTS_SABOTAGE=localrange the
// interval claimed is its lower bound alone ([3, 3]): the store witness of
// the local must report the first other value.
// A loop bound the compiler cannot fold keeps the loop as written.
const RUN = Math.random() < 2 ? 1 : 0;
const table: number[] = [];
for (let i = 0; i < 64; i++) table.push(i * 7);
function pick(i: number): number {
  const k = (i & 15) + 3;
  return table[k * 2 + 1] ?? -1;
}
let s = 0;
for (let i = 0; i < 40 * RUN; i++) s += pick(i);
console.log(s);
