// #9725: large integral doubles must zero-fill unrepresented radix digits.
// Include values on both sides of 2^53, large exponents, both signs, and
// every radix so the power-of-two and decimal paths remain covered.
function dynamic(value: any, key: string, radix: number): string {
  return value[key](radix);
}

const values: number[] = [
  0, -0, 0.1, 10.5, 255, 1e15,
  9007199254740991, 9007199254740992, 9007199254740994,
  18014398509481984, 1e21, 1e30, 1e100, Number.MAX_VALUE,
];
for (const value of values) {
  for (let radix = 2; radix <= 36; radix++) {
    console.log(value.toString(radix));
    console.log((-value).toString(radix));
    console.log(dynamic(value, "toString", radix));
  }
}

console.log((1e21).toString(36));
console.log((1e21).toString(7));
console.log((1e30).toString(36));
console.log(dynamic(new Number(1e21), "toString", 36));
