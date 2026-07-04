// Issue #5927 (companion to #5918/#5922/#5924) — a PLAIN named import's bare
// name and a NAMESPACE MEMBER's bare name share the same flat
// `import_function_prefixes` / `import_function_origin_names` maps. Since a
// plain import has NO other resolution path (a bare call has no namespace to
// scope against), it must always win, but pre-fix a namespace's `.insert()`
// could silently overwrite it depending on import-statement order.
//
// This shape is lifted from a real-world source compile of `sst/opencode`:
// `provider.ts` does `import { omit } from "remeda"` (a plain import) AND
// `import { Context } from "effect"` (a namespace-reexport import), and
// effect's real `Context.ts` ALSO exports a member literally named `omit`.
// Order A: the plain import is processed first, the namespace second.
import { omit } from "./fixtures/issue_5927_pkg/plain_mod.ts"
import { NsA } from "./fixtures/issue_5927_pkg/barrel.ts"

console.log(omit())
console.log(NsA.omit())
