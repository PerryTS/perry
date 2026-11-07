// `x.length` is a Number by the value `x` holds, never by its annotation:
// an element of a `string[]` written through `any` can be a Number, whose
// `length` is undefined, so a Number accumulator that adds it becomes NaN.
// Proven strings and arrays keep a Number `length`.
const lied: string[] = ["ok", "still a string"];
(lied as any)[1] = 42;
function sum(): number {
  let t = 0;
  for (let k = 0; k < 2; k++) t = t + lied[k].length;
  return t;
}
function acc(): number {
  let t = 0;
  for (let k = 0; k < 2; k++) t += lied[k].length;
  return t;
}
console.log(sum(), acc(), String(acc()));
const words = ["a", "bb", "ccc"];
let total = 0;
for (const w of words) total += w.length;
console.log(total, "literal".length * 2, [1, 2, 3].length + 1);
