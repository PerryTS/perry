Fixed a miscompile where a class receiver typed as a `Union` (`Foo |
undefined`, or a generic like cheerio's `Cheerio<AnyNode> | undefined`)
had a method call folded to the dense `Array.prototype` fast path whenever
the method name collided with a real Array method (`find`, `map`,
`filter`, `some`, `every`, `forEach`, `reduce`, `reduceRight`, `join`,
`findIndex`, `findLast`, `findLastIndex`, `push`). Three separate guards
in `crates/perry-hir/src/lower/expr_call/{local_array_methods,array_only_methods}.rs`
matched `Type::Named`/`Type::Generic` directly but had no `Type::Union`
arm, so a union-typed class receiver read as "not a class instance" and
the fold went ahead — calling the user's argument as an `Array.prototype`
callback, or reading the class instance's `ObjectHeader` as an
`ArrayHeader`.

cheerio (`cheerio.load(html)("selector")`) hit this on its most basic
operation: `load.ts`'s `searchContext.find(search)`, where `searchContext:
Cheerio<AnyNode> | undefined` and `find` is cheerio's own CSS-selector
method, mixed onto `Cheerio.prototype` at runtime — every call threw
`TypeError: string "..." is not a function`. `cheerio@1.2.0` now compiles
and runs end-to-end, byte-identical to Node.

All three guards now recurse through `Type::Union` (nested unions
included) using the same per-variant test they already applied to a bare
receiver, matching the idiom the surrounding code already used in six
other places in `local_array_methods.rs`. #10796
