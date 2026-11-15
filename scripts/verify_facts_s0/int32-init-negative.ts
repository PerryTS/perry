// `d` is a `const` of a loop in the module body, with an int use (`=== 7`):
// the facts give it `I32` as soon as its interval claims int32, and it takes
// the canonical i32 slot in module init, a body with no proven-`let` set.
// Under PERRY_FACTS_SABOTAGE=range the `+` interval rule drops its right
// operand and claims exactly that: the witness of the value that enters
// the slot must report it.
const RUN = Math.random() < 2 ? 1 : 0;
const outv: number[] = [];
for (let i = 0; i < 4 * RUN; i++) {
  const d = i + 3000000000;
  let h = 0;
  if (d === 7) h = 1;
  outv.push(h + d * 0.5);
}
console.log(outv.join(","));
