// A `+` of two calls the facts prove strings is lowered as a string
// concatenation (`expr::facts_string`). `pick` returns a string or a number,
// so its calls are no string calls. Under PERRY_FACTS_SABOTAGE=stringcall
// the facts claim every call a string call: the operand witness must report
// the number.
// A bound the compiler cannot fold keeps the calls as written.
const RUN = Math.random() < 2 ? 1 : 0;
function pick(i: number): string | number {
  if (i % 3 === 0) return "s" + i;
  return i * 2;
}
let text = "";
let sum = 0;
for (let i = 0; i < 6 * RUN; i++) {
  const v = (pick(i) as any) + (pick(i + 3) as any);
  if (typeof v === "string") text += v;
  else sum += v;
}
console.log(text, sum);
