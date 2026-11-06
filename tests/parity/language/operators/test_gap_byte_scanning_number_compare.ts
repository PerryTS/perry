// Strict comparison must preserve both Number representations and never coerce.
let coerced = 0;
const values: any[] = [undefined, null, false, true, "", "92", 92, -0, NaN,
  Infinity, -Infinity, 2147483647, -2147483648, 2147483648, -2147483649,
  92.25, {}, { valueOf() { coerced++; return 92; } }, 92n, Symbol("n")];
const targets = [-0, 1, -1, 92, 92.25, 2147483647, -2147483648,
  2147483648, -2147483649, NaN, Infinity, -Infinity];
function literal(value: any) {
  return [value === 92, 92 === value, value !== 92, 92 !== value,
    value === -0, value === 92.25, value === 2147483647,
    value === -2147483648, value === 2147483648, value === Infinity];
}
function variable(value: any, target: any) {
  const number = Number(target);
  return [value === number, number === value, value !== number, number !== value];
}
for (let i = 0; i < values.length; i++) {
  console.log(i, JSON.stringify(literal(values[i])));
  for (let j = 0; j < targets.length; j++)
    console.log(i, j, JSON.stringify(variable(values[i], targets[j])));
}
console.log("coercions", coerced);
const bytes = new Uint8Array([34, 92, 0, 255]);
for (let i = -1; i < 6; i++) console.log(i, bytes[i] === 34, bytes[i] !== 92);
