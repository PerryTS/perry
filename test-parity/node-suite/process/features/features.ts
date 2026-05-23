// process.features — object of boolean capability flags. Consumers
// branch on individual fields (TLS variant, openssl/boringssl,
// IPv6 availability). Regression cover for #1378 (Perry was
// returning a 0 sentinel, so any field read was undefined).
//
// Cross-runtime parity is checked on _shape_ rather than the specific
// flag values: Perry's TLS/IPv6/typescript story doesn't have to match
// Node bit-for-bit, only the field types.
const f = process.features;
console.log("typeof:", typeof f);
console.log("tls type:", typeof f.tls);
console.log("ipv6 type:", typeof f.ipv6);
console.log("openssl_is_boringssl type:", typeof f.openssl_is_boringssl);
console.log("tls_alpn type:", typeof f.tls_alpn);
console.log("inspector type:", typeof f.inspector);
