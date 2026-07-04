// Issue #5924 (companion to #5922/#680/#678) — `import_function_origin_names`
// (issue #678's symbol-suffix-override map for re-export renames) is FLAT,
// keyed only by the member's bare name. A NAMED import of a value the
// source module re-exports as a NAMESPACE (`import { NsA, NsB } from
// "./barrel.ts"` where barrel.ts has `export * as NsA from "./ns_a.ts"`
// and `export * as NsB from "./ns_b.ts"`) processes each namespace target
// into this file's SHARED maps, one specifier at a time.
//
// ns_a.ts re-exports its `Service` member under a RENAME (the real
// identifier is "a"), so processing NsA inserts
// `import_function_origin_names["Service"] = "a"`. ns_b.ts exports
// `Service` DIRECTLY (no rename needed), so processing NsB inserts
// NOTHING — but the earlier (wrong-for-NsB) "Service" -> "a" entry from
// NsA's turn survives untouched in the shared flat map. `NsB.Service()`
// then resolves via the contaminated entry and looks for a symbol named
// "a" in ns_b.ts, which doesn't exist there.
//
// This shape is lifted directly from `effect`'s real barrel structure,
// found via a real-world source compile of `sst/opencode`: `provider.ts`
// does `import { Effect, Layer, Context, Schema, Types } from "effect"`
// and calls `Context.Service<...>()(...)` — `Effect`'s own re-exported
// `Service` (processed earlier in the same import) clobbered `Context`'s
// direct (unrenamed) `Service` export, producing an undefined
// `perry_fn_..._Context_ts__a` symbol at link time instead of the correct
// `..._Context_ts__Service`.
import { NsA, NsB } from "./fixtures/issue_5924_pkg/barrel.ts"

console.log(NsA.Service())
console.log(NsB.Service())
