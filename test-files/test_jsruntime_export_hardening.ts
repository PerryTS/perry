import {
  ACCESSOR_DATA,
  ARRAY_DATA,
  CUSTOM_PROTO_DATA,
  DEEP_DATA,
  FUNCTION_DATA,
  MUTABLE_DATA,
  PROMISE_DATA,
  PROXY_DATA,
  SAFE_DATA,
  SYMBOL_DATA,
  TAMPERED_INTRINSICS_DATA,
  mutateMutable,
  readAccessorTwice,
  readArray,
  readCustomProto,
  readDeep,
  readSymbol,
  readMutable,
  readProxyTwice,
  readTamperedIntrinsics,
} from "./fixtures/jsruntime_export_hardening.js";

console.log(
  "safe:",
  SAFE_DATA.label,
  SAFE_DATA.count,
  SAFE_DATA.nested.flag,
  SAFE_DATA.nested.text,
);

mutateMutable();
console.log("mutable:", readMutable(MUTABLE_DATA));

console.log("accessor:", readAccessorTwice(ACCESSOR_DATA));
console.log("custom-proto:", readCustomProto(CUSTOM_PROTO_DATA));
console.log("proxy:", readProxyTwice(PROXY_DATA));
console.log("array:", readArray(ARRAY_DATA));
console.log("function:", FUNCTION_DATA());
console.log("promise:", await PROMISE_DATA);
console.log("symbol:", readSymbol(SYMBOL_DATA));
console.log("deep:", readDeep(DEEP_DATA));
console.log("tampered-intrinsics:", readTamperedIntrinsics(TAMPERED_INTRINSICS_DATA));
