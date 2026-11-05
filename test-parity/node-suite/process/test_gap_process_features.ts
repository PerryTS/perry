// process.features must be a real, non-null object of capability flags.
// Node exposes boolean flags plus a `typescript` string. We assert the
// deterministic structural invariants (object-ness, presence, and the JS
// type of each flag) rather than the concrete boolean values, which
// legitimately differ between a Perry build and a Node build.
const f = process.features;

console.log("typeof:", typeof f);
console.log("non-null:", f !== null);

// Boolean capability flags Node always exposes.
const boolKeys = [
  "inspector",
  "debug",
  "ipv6",
  "tls",
  "tls_alpn",
  "tls_ocsp",
  "tls_sni",
  "uv",
  "cached_builtins",
  "require_module",
];
for (const k of boolKeys) {
  console.log(k, "is boolean:", typeof f[k] === "boolean");
}

// `typescript` is a string in modern Node (the strip/transform mode).
console.log("typescript is string:", typeof f.typescript === "string");

// Chained reads must not crash.
console.log("chained tls ok:", f.tls === true || f.tls === false);
