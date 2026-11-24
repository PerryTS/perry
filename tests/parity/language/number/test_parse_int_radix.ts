// parseInt(string, radix): the radix selects the base; an omitted or zero radix reads a
// `0x` prefix as hexadecimal; a radix outside 2..=36 gives NaN; a radix computed at run
// time works like a literal one.
const r: number = Number("16");
console.log(parseInt("ff", 16), parseInt("0x1F", 16), parseInt("777", 8), parseInt("9", 8));
console.log(parseInt("z", 36), parseInt("101", 2), parseInt("ff", r), parseInt("  -7f", 16));
console.log(parseInt("0x1F"), parseInt("0x1F", 0), parseInt("42"), parseInt("12", 10));
console.log(parseInt("ff", 1), parseInt("ff", 37), parseInt("10", 2.9), parseInt("10", "8" as any));
