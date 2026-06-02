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

import { getAccessToken, getUserPlan } from "./fixtures/issue_1013/user_lookup.ts";

async function fetchA(): Promise<string> {
    return "hello-from-A";
}

async function fetchB(): Promise<{ plan: string }> {
    return { plan: "pro" };
}

async function main() {
    // Same-file literal-return shape — covers the dispatch bug.
    const [a, b] = await Promise.all([fetchA(), fetchB()]);
    console.log("a:", JSON.stringify(a), "typeof:", typeof a);
    console.log("b:", JSON.stringify(b), "typeof:", typeof b);
    console.log("b.plan:", b?.plan);

    // Cross-module async helpers returning property reads off the
    // resolved value — the exact shape from the original gscmaster-api
    // repro. Covers the full #1013 surface, not just the dispatch
    // path. (See PR #1064 review feedback.)
    const [accessToken, userPlan] = await Promise.all([
        getAccessToken("u1"),
        getUserPlan("u1"),
    ]);
    console.log("accessToken:", JSON.stringify(accessToken), "typeof:", typeof accessToken);
    console.log("userPlan:", JSON.stringify(userPlan), "typeof:", typeof userPlan);
    console.log("userPlan.plan:", userPlan?.plan);
}

main();
