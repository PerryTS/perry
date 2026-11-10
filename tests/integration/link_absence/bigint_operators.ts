// HWS hook: js_runtime_install_bigint
// HWS prefixes: num_bigint::|perry_runtime::bigint::|js_bigint_
// HWS companion: bigint
// HWS argv: ["12345678901234567890"]
const a: any = BigInt(process.argv[2]);
const b: any = 3n;
console.log(String(a + b), String(a - b), String(a * b));
console.log(String(a / b), String(a % b), String(b ** 17n));
console.log(String(a & b), String(a | b), String(a ^ b));
console.log(String(a << b), String(a >> b), String(~a), String(-a));
console.log(a.toString(16), a > b, a == "12345678901234567890");
let n: any = b; console.log(String(n++), String(++n));
console.log(String(BigInt.asIntN(8, 255n)), String(BigInt.asUintN(8, -1n)));
console.log(new Map([[a, "ok"]]).get(BigInt("12345678901234567890")));
const view=new BigInt64Array(2); view[0]=123456789n; view[1]=-7n;
console.log(String(view[0]+view[1]));
