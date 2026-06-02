// Issue #431: cross-module class-name collision emitted method bodies under
// the wrong module prefix. When two TypeScript modules each declared a
// class with the same name, perry's codegen mangled the consumer module's
// method bodies with the *defining* module's prefix (the first
// registration that won the race in `imported_class_prefix`), while the
// dispatch-table builder still expected the consumer's prefix. Clang
// rejected the IR with:
//
//   error: use of undefined value '@perry_method_<consumer>_ts__<C>__<m>'
//
// Surfaced on Effect 3.21+ across 8 modules (Effect's `Class`,
// `Refinement`, `Composite`, `ParseError`, `PropertySignatureTransformation`,
// `DroppingStrategy` are reused across multiple files). Cross-module
// sibling of #336 (intra-module collision, fixed in v0.5.405 via the
// `_<n>` discriminator on `@perry_class_keys_*`).
//
// Fix in `crates/perry-codegen/src/codegen.rs::compile_module` — when
// populating `imported_class_prefix` (the `name → source-module-prefix`
// map that `compile_method` consults to mangle class method symbols),
// skip imports that collide by name with a LOCAL class. The local class
// shadows the import in `class_table`; without this skip, the lookup
// at line 1106 returned the imported module's prefix for a local class
// and `compile_method` emitted the local methods under the wrong prefix
// while the dispatch table still expected the local one — leaving
// `@perry_method_<local>__<C>__<m>` undefined at link time.

import { useStrategy, probeSize } from "./fixtures/issue_431_pkg/b.ts";

const ATypeId: unique symbol = Symbol.for("ATypeId-issue-431");
const aVar = { _A: (_: never) => _ };

// Same class NAME as `b.ts` defines. The two definitions are intentionally
// inverted: A's `handleSurplus` returns `length === 0`, B's returns
// `length > 0`; A's `surplusSize` returns 0, B's returns 1. The output
// proves that each module's call sites resolve to ITS OWN class body,
// not the other module's.
export class DroppingStrategy<in out A> {
  readonly [ATypeId] = aVar;
  surplusSize(): number {
    return 0;
  }
  handleSurplus(elements: Array<A>): boolean {
    return elements.length === 0;
  }
}

const a = new DroppingStrategy<number>();
console.log("a.handleSurplus:", a.handleSurplus([1, 2, 3]));
console.log("a.surplusSize:", a.surplusSize());
console.log("b.handleSurplus:", useStrategy());
console.log("b.surplusSize:", probeSize());
