// Issue #310: `export * as Foo from "./Foo"` (ES2020 namespace re-export)
// was silently dropped by `ExportNamed`'s `if let ExportSpecifier::Named`
// filter at HIR lowering. The re-exported file never entered the module
// graph, the consumer's `import { Foo } from "pkg"` resolved to a stale
// no-op binding, and every `Foo.<member>` access lowered to 0 — silent-
// correctness bug, exit 0, no diagnostics. The reproducer in the issue
// was Effect (29k-line `Effect.ts` re-exported via `export * as Effect
// from "./Effect.js"` from the package's `index.ts`); this file
// exercises the same code path with relative imports against a tiny
// fixture under `tests/fixtures/programs/fixtures/issue_310_pkg/`.
//
// Fix lands in three pieces (v0.5.404):
//   - `Export::NamespaceReExport { source, name }` HIR variant.
//   - SWC's `ExportSpecifier::Namespace` lowers into it.
//   - `collect_modules` traverses it (file enters the module graph).
//   - Consumer-side dispatch in `compile.rs` detects when an import's
//     exported name matches a `NamespaceReExport` in the source HIR
//     and registers the local as a namespace import — same code path
//     `import * as Foo from "pkg/Foo"` would have used.

import { Foo, Bar } from "../../../fixtures/programs/fixtures/issue_310_pkg/index.ts";

// Case 1: function call through the namespace alias.
const a = Foo.succeed(42);
console.log("case1:", a);

// Case 2: multi-arg function call through the namespace alias.
const b = Foo.add(7, 35);
console.log("case2:", b);

// Case 3: chained namespace calls (proves the dispatch isn't a one-shot
// fluke that worked for the first member access only).
const c = Foo.runSync(Foo.succeed(100));
console.log("case3:", c);

// Case 4: SECOND namespace from the SAME re-exporter — confirms multi-
// alias support per package (Effect re-exports dozens this way).
const d = Bar.greet("world");
console.log("case4:", d);
console.log("case5:", Bar.shout("ok"));

// Case 6: namespace member used inside an expression context.
const sum = Foo.add(Foo.succeed(10), Bar.greet("x").length);
console.log("case6:", sum);

console.log("done");
