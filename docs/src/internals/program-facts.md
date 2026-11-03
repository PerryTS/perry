# Program facts

`perry-wpfacts` analyzes lowered HIR value flow, not declared TypeScript types. The default driver solves per-module units and exchanges contracts to a fixed point. `PERRY_FACTS_UNITS=0` selects the whole-program reference solve; `PERRY_PROGRAM_FACTS=0` disables the pass. Work or memory limits discard unproved results and preserve generic behavior.

Facts describe field writers, definite birth keys, value kinds, escaping objects, stable field prefixes, fixed layouts, receiver sets and function targets. A known field offset does not alone prove property presence: declared or reserved fields may still contain a hole. Typed-array facts also require evidence about exposure, detachment and resizing.

Codegen consumes these facts today. A proven receiver and field can use a fixed-offset load or store; multiple layouts may require class selection. Uncertain receivers retain guards and a complete generic fallback. Interpreted code is modeled as unknown behavior, with prototype-write guards where required. Pointer stores still need GC barriers, and managed values still need roots.

Local initializer evidence and whole-program facts are distinct analyses. An annotation alone cannot justify unboxed arithmetic, a field load or removal of a GC root. Native scalar and packed-array paths need their own representation, mutation and escape proofs; there is no blanket promise that all TypeScript values remain boxed or become unboxed.

Use `PERRY_FACTS_DUMP=<path>` for analysis and elision records, `PERRY_FACTS_CONSUMED=<path>` for source sites consuming facts, and `PERRY_FACTS_REPORT=1` for solver statistics. `PERRY_FACTS_ELIDE=0` keeps analysis but disables its check elision. Contract builds (`PERRY_CHECK_CONTRACTS=1`, runtime `contract-checks`) validate claims against actual objects. [Fallback diagnostics](fallback-diagnostics.md) explain retained generic operations.

The implementation starts in `crates/perry/src/commands/compile/program_facts.rs` and `crates/perry-wpfacts/src/`. Codegen receives `ProgramFactsView` in `crates/perry-codegen/src/codegen/program_facts_view.rs`; field consumers live in `crates/perry-codegen/src/expr/wpfacts_access.rs`. Fact digests participate in object-cache identity.
