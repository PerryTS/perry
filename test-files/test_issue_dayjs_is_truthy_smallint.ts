// Regression: js_is_truthy must not SIGSEGV when the f64 bit pattern lands in
// what looks like the legacy "raw string pointer" range. Surfaced by dayjs
// where a utility-object call funneled bits=0x646e (a plain integer) into
// js_is_truthy, which dereferenced it as *StringHeader.
//
// 0x646e == 25710 is the canonical repro: small enough to historically match
// the >0x1000 raw-pointer heuristic, large enough to be a real JS integer.
const x: any = 0x646e;
console.log("truthy:", x ? "t" : "f");

const y: any = 0.0;
console.log("zero:", y ? "t" : "f");

// Extra coverage: another small integer in the dangerous range.
const z: any = 1;
console.log("one:", z ? "t" : "f");
