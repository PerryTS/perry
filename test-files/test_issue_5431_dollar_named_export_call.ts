// Issue #5431 — calling a `$`-prefixed exported function across a module
// boundary returned `undefined` instead of running the body. Surfaced by
// zod v4: `z.string()` calls `core._string(ZodString, params)` where
// `ZodString = /*@__PURE__*/ core.$constructor("ZodString", ...)`. The
// `$constructor` call returned `undefined`, so `ZodString` was `undefined`
// and `new ZodString(...)` threw "TypeError: undefined is not a constructor"
// (api.ts:65).
//
// Root cause: a function body whose name contains a non-`[A-Za-z0-9_]` char
// is emitted under the injective `sanitize_member` symbol, but the
// exported-function alias loop skipped emitting a forwarding alias whenever
// `local == exported`. The #461 named-export stub loop then claimed the
// plain-`sanitize` symbol with an undefined-returning body, and every
// cross-module call resolved to that stub. Fix: emit the forwarding alias
// whenever the plain-`sanitize` symbol differs from the real body symbol,
// regardless of whether the local and exported names match.

import * as ns from "./fixtures/issue_5431_pkg/index.ts";
import { $tag, $constructor } from "./fixtures/issue_5431_pkg/index.ts";

// Namespace-member call of a `$`-named function returning a value.
console.log("ns.$tag:", ns.$tag("a"));
// Named-import call of the same.
console.log("$tag:", $tag("b"));

// `$constructor` returns a function — the reference must resolve AND the
// call must run the body (pre-fix: typeof was "function" but the result
// was undefined).
const Ctor = $constructor("Foo", (inst: any) => {
  inst.kind = "foo";
});
console.log("typeof Ctor:", typeof Ctor);
const inst = new Ctor({ x: 1 });
console.log("inst.kind:", inst.kind);
console.log("inst._zod.def.x:", inst._zod.def.x);

// The zod-shaped path: `export const ZodString = core.$constructor(...)`
// re-exported through a barrel, then consumed via `new`.
console.log("typeof ns.ZodString:", typeof ns.ZodString);
const s = ns.string();
console.log("s.kind:", s.kind);
console.log("s._zod.def.type:", s._zod.def.type);
