// HWS hook: js_runtime_install_bigint
// HWS prefixes: num_bigint::|perry_runtime::bigint::|js_bigint_
// HWS companion: bigint
// HWS argv: ["BigInt"]
const B:any=globalThis[process.argv[2]];
const C:any=global[process.argv[2]];
console.log(String(B("12345678901234567890")+C(1)));
