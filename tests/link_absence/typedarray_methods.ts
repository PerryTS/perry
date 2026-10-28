// HWS companion: typedarray
// HWS hook: js_runtime_install_typedarray
// HWS prefixes: perry_runtime::typedarray::|js_typedarray_|js_typed_array_
const Kind=Int32Array;const values=new Kind({0:3,1:-1,2:2,length:3});
console.log(Array.from(values).join(","));console.log(values.map(value=>value+1).join(","));console.log(values.toSorted().join(","));
const big=new BigInt64Array([3n,-1n,2n]);console.log(big.toSorted().join(","));
