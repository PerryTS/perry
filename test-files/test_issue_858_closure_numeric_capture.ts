// Regression test for issue #858 (downstream #859):
// closure-captured numeric params reach an object-literal method body.
//
// Pre-fix, the inliner's `try_inline_simple_call` Pattern 1 substituted a
// literal call-site arg (`makeDT(2026)` -> `y === Integer(2026)`) directly
// into the nested closure's body — rewriting `LocalGet(y) -> Integer(2026)`
// and emptying the closure's `captures` list at that call site. Codegen
// then emitted the closure with zero capture slots, while the compiled
// closure body (keyed by `func_id`, collected from the original
// uninlined occurrence) still read capture slot 0 expecting `y`. Reading
// the uninitialized slot returned 0, so `Date.UTC(y, 4, 15, ...)`
// computed `Date.UTC(0, 4, 15, ...)` — year 0, getTime() ≈ -6.2e13.
//
// The fix is in `crates/perry-transform/src/inline.rs`:
// pre-walk the inlined body for closure captures, and for any param
// captured by a nested closure, materialize the arg as a fresh `Let`
// (so the closure body keeps a `LocalGet(fresh)` reference and
// `captures: [fresh]` stays non-empty) instead of substituting the
// literal in place. Symmetric fix applied to all four inliner paths:
// fn-call simple, method-call simple, void-method simple, and the
// multi-stmt `try_inline_call` fn / method branches.

// 1. The exact repro from the issue: numeric param captured by a method
//    shorthand inside the returned object literal.
function show(label: string, d: Date) {
    console.log(label, "getTime=", d.getTime(), " iso=", d.toISOString());
}
function makeDT(y: number) {
    return {
        toDate(): Date {
            return new Date(Date.UTC(y, 4, 15, 17, 29, 35, 402));
        },
    };
}
const d1 = makeDT(2026).toDate();
console.log("INLINE:", d1.getTime());
show("HELPER:", d1);

// 2. String capture — the dangerous trivial exprs are
//    Integer/Number/Bool/String/Null/Undefined; string literal coverage.
function makeGreeter(name: string) {
    return {
        say(): string {
            return "hello, " + name;
        },
    };
}
console.log("STR:", makeGreeter("ralph").say());

// 3. Multiple primitives captured at once.
function makeAdder(a: number, b: number, label: string) {
    return {
        sum(): string {
            return label + ":" + (a + b);
        },
    };
}
console.log("MULTI:", makeAdder(2, 3, "two+three").sum());

// 4. Arrow-form closure (not method shorthand) — same inliner path.
function makeArrow(y: number) {
    return () => y * 7;
}
console.log("ARROW:", makeArrow(6)());

// 5. Closure capture nested inside an object literal *value* expression.
function makeBox(n: number) {
    return {
        wrap: (label: string) => label + ":" + (n * 2),
    };
}
console.log("ARROW-IN-OBJ:", makeBox(21).wrap("doubled"));

// 6. Captures used in array .map — passes through the same inliner.
function makeMapper(factor: number) {
    return {
        run(xs: number[]): number[] {
            return xs.map((x) => x * factor);
        },
    };
}
console.log("MAP:", makeMapper(10).run([1, 2, 3]).join(","));

// 7. Multi-statement body (Pattern 2 of try_inline_simple_call):
//    a const Let then a Return-with-closure. Pre-fix this had the same
//    problem when the closure captured a param.
function makeMulti(y: number) {
    const k = "yval";
    return {
        get(): string {
            return k + "=" + y;
        },
    };
}
console.log("MULTI-STMT:", makeMulti(123).get());
