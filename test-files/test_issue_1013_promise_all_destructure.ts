// #1013: `const [a, b] = await Promise.all([fn1(), fn2()])` silently
// produced `a === undefined` / `b === undefined` under Perry while
// sequential awaits worked. Root cause was the HIR shape for
// `Promise.all`: post-#973 (constructor-property) lowering routed the
// bare `Promise` ident-as-value to `PropertyGet { GlobalGet(0),
// "Promise" }`, so a static-method call `Promise.all(...)` became
// `PropertyGet { PropertyGet { GlobalGet(0), "Promise" }, "all" }`
// (two levels deep). The codegen Promise-static dispatch only matched
// the bare-`GlobalGet` receiver, so the call fell through to
// `js_native_call_method("all", ...)` against the Promise constructor
// — which returned `0.0` (no such method on a function value), making
// the awaited result a number and the destructure indexes read 0 /
// undefined.
//
// #1007 collapses the member-object reroute back to `GlobalGet(0)`
// when the original ident name matches the property (Promise.all,
// Number.parseFloat, …) so the codegen's existing fast-path fires.
// This test pins the byte-for-byte node match so a future regression
// of either the HIR collapse or the codegen `is_global_constructor_expr`
// helper fails immediately.

async function fetchA(): Promise<string> {
    return "hello-from-A";
}

async function fetchB(): Promise<{ plan: string }> {
    return { plan: "pro" };
}

async function main() {
    const [a, b] = await Promise.all([fetchA(), fetchB()]);
    console.log("a:", JSON.stringify(a), "typeof:", typeof a);
    console.log("b:", JSON.stringify(b), "typeof:", typeof b);
    console.log("b.plan:", b?.plan);
}

main();
