// Issue #5916: re-exporting a NAMESPACE-valued binding through a barrel
// (`export { Token } from "./mod"`, where `mod` declares `export * as Token`)
// must resolve to the namespace, not to a bare function symbol.
//
// Pre-fix this did not even link:
//   Undefined symbols for architecture arm64:
//     "___perry_wrap_perry_fn_<barrel>__Token", referenced from: _main
// because the consumer-side namespace detection only looked at the barrel's own
// exports and never followed the `export { … } from` hop to the module that
// actually declares the namespace.
//
// Importing the same names DIRECTLY from the declaring module (no barrel hop)
// always worked — the bug is specifically about surviving the re-export hop.

import { Token, estimate } from "./namespace_reexport_barrel.ts";

// Namespace-valued re-export: member access must dispatch through the namespace.
console.log(Token.estimate("hello world"));

// Ordinary value re-export across the same hop (this already worked — guard it).
console.log(estimate("hello world"));
