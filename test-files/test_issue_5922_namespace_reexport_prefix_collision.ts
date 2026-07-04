// Issue #5922 (companion to #5918/#680) — a NAMED import of a value that
// the source module re-exports as a NAMESPACE (`import { Context, Option }
// from "./barrel.ts"` where barrel.ts has `export * as Context from
// "./context.ts"` and `export * as Option from "./option.ts"`) registered
// every member of the target namespace into the FLAT
// `import_function_prefixes` map, keyed only by the member's bare name —
// never into the collision-safe `namespace_member_prefixes` map (keyed by
// `(namespace_local, member_name)`) that `random.make` vs `tracer.make`
// disambiguation (#680) already relies on.
//
// context.ts and option.ts both export a member literally named "a".
// Pre-fix, whichever namespace registered last in this file's import loop
// won the flat map for ALL of "a" — so both `Context.a()` and `Option.a()`
// resolved to the SAME origin function.
//
// This shape is lifted directly from `effect`'s real barrel structure,
// found via a real-world source compile of `sst/opencode`, whose
// `provider.ts` does `import { Effect, Layer, Context, Schema, Types }
// from "effect"` and `cmd/providers.ts` does `import { Option } from
// "effect"` — both `Context.ts` and `Option.ts` are reached via effect's
// namespace-reexport barrel.
import { Context, Option } from "./fixtures/issue_5922_pkg/barrel.ts"

console.log(Context.a())
console.log(Context.make())
console.log(Option.a())
console.log(Option.make())
