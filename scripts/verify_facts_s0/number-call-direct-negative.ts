// `label()` returns a string, and it is called directly (no receiver). Under
// PERRY_FACTS_SABOTAGE=numbercall the facts claim every call returns a
// Number: the call-result witness of the Number-call proof must report it.
// A loop bound the compiler cannot fold keeps the call one HIR call.
const RUN = Math.random() < 2 ? 1 : 0;
function label(v: number): string {
  let r = "n";
  for (let k = 0; k < 2 * RUN; k++) r += v + k;
  return r;
}
const out: number[] = [];
let text = "";
for (let i = 0; i < 3 * RUN; i++) text = label(i);
out.push(text.length + 1);
console.log(out[0]);
