fix(wpfacts): resolve `export … from` sources, so calls through a re-exported namespace reach the module's functions

`export * as X from "./x"`, `export { a } from "./x"` and `export * from "./x"`
declare no import of `./x`. The whole-program facts resolved a re-export's
source only through the module's own imports, so each of these exports was ⊤
("unresolved re-export source"). A call like `X.f(lit)` then went to an
unknown callee: `lit` escaped, but `f` never saw it as an argument and was
never marked as reachable from unknown code. When `f` also had a known caller,
its parameter's class set held only that caller's literals, and a property
read on the parameter was proven at the wrong slot.

opencode v2.1.2 (`util/effect/layer-node.ts` re-exports its own namespace and
`makeProvider` has an in-module caller): every `makeGlobalNode({ service, deps,
layer })` read `deps` as `layer`. The server's skill plugin failed with
`SchemaError(Expected Skill.Info)` (Skill.Info.make returned a function) and the
TUI showed "1 plugin failed"; a module subset failed with "A layer node requires
an Effect Layer".

The driver now resolves every re-export source the way init order does and
hands the analysis the targets that are program modules
(`program_facts::export_sources`); the analysis prefers that resolution and
falls back to an import of the same source. Gap test
`test_gap_namespace_reexport_member_calls` (self-namespace, barrel namespace,
named and `export *` re-exports) matches node.
