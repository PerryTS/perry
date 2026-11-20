// Helper for test_gap_namespace_reexport_barrel.ts — issue #5916.
//
// A barrel that forwards BOTH an ordinary value (`estimate`) and a
// NAMESPACE-valued binding (`Token`) across one `export { … } from` hop.
//
// Pre-fix, the consumer-side namespace detection only inspected THIS module's
// own exports for a `NamespaceReExport`. It found a plain `ReExport`, fell
// through to the ordinary value path, and `Token.estimate(…)` in the consumer
// lowered to a `StaticMethodCall` against
// `__perry_wrap_perry_fn_<this module>__Token` — a wrapper symbol nobody emits,
// so the whole program failed to LINK. The ordinary `estimate` re-export across
// the same hop always linked fine; it is specifically the namespace-valued
// binding that needs the namespace routing.
export { Token, estimate } from "./namespace_reexport_selfns.ts";
