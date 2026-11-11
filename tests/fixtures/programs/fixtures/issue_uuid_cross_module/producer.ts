// Named-default-function regression: `export default function rng() { ... }`
// (named binding). Pre-fix the HIR lowerer's `ExportDefaultDecl::Fn` branch
// took the `ident.is_some()` path, pushed an `Export::Named` entry, and
// dropped the function body entirely (the `// TODO: properly lower function
// expression` comment was the live bug). The consumer's
// `import rng from "./producer"; rng()` resolved through
// `ExternFuncRef { name: "default" }` to a never-emitted
// `perry_fn_<src>__default` symbol, returning undefined at runtime — which
// is exactly the shape that broke uuid's `v4()` via `rng.js`'s
// `export default function rng() { return crypto.getRandomValues(rnds8); }`.
export default function rng() {
    return new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
}
