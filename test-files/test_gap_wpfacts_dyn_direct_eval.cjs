// Decision 150: a direct eval of source built at run time reads and writes
// the enclosing function's locals (its scope escapes to interpreted code).
// `readN` (in the helper module, out of the eval's lexical reach) holds
// `Box` instances, and whatever the eval stored into the caller's `b`
// (⊤): its read is CHECKED, so the literal the eval made, whose `n` is not
// at `Box`'s slot, takes the generic read.
const { readN, total } = require("./gap_wpfacts_dyn_direct_eval_helper.cjs");

function run(code) {
  let b = { n: "local" };
  const before = readN(b);
  eval(code);
  return [before, readN(b), b.n, total()].join("|");
}

console.log(total());
const parts = ["b = { pad: 0, ", "n: 'from eval' }"];
console.log(run(parts.join("")));
console.log(run(process.argv.length > 99 ? "" : "b.n = b.n + 40"));
