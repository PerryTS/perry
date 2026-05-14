// #679 Phase 2: JSX audit for perry/tui.
//
// Verifies what JSX shapes lower + run end-to-end:
//
//  1. User-component dispatch: `<App />` calls App(props). The runtime
//     `js_jsx` adapter (perry-runtime/src/jsx.rs) recognises the type
//     as a closure pointer and invokes it.
//  2. JSX-as-call-expression: `const w = <Comp x={1} />` is just an
//     expression returning whatever Comp returns.
//
// What does NOT yet lower cleanly through JSX (deferred follow-up):
//
//  3. Built-in widget intrinsics: `<Box flexDirection="row">...</Box>`
//     and `<Text color="red">...</Text>` lower to `jsx(Box, props)` and
//     today fall through to TAG_UNDEFINED. The function-call form
//     `Box({ flexDirection: "row" }, [Text("…", { fg: "red" })])` still
//     works as documented in #358 Phase 3+.
//
// Workaround: stay on function-call form for Box/Text intrinsics until
// the Box/Text JSX compile-time rewriter lands.

import { Box, Text, render } from "perry/tui";

function Hello(props: { name: string }) {
    return Text("hello " + props.name);
}

// 1. User component dispatch via JSX.
const greeting = <Hello name="ralph" />;
render(Box([greeting]));
console.log("\n--- user-component JSX ok ---");

// 2. JSX with no props.
function Tag() {
    return Text("[tag]");
}
const t = <Tag />;
render(Box([t]));
console.log("\n--- no-props JSX ok ---");

/*
@covers
crates/perry-runtime/src/jsx.rs:
  - js_jsxs
*/
