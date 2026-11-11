// HWS argv: ["7"]
const a: any = {valueOf() { return Number(process.argv[2]); }};
const b: any = 3;
console.log(a + b, a - b, a * b, a / b, a % b, a ** b);
console.log(a & b, a | b, a ^ b, a << b, a >> b, ~a);
console.log(String(a), a < b, a == "7");
