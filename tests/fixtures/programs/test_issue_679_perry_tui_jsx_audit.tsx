// #679 Phase 2: JSX audit for perry/tui.
//
// Verifies what JSX shapes lower + run end-to-end:
//
//  1. User-component dispatch: `<App />` calls App(props). The runtime
//     `js_jsx` adapter (perry-runtime/src/jsx.rs) recognises the type
//     as a closure pointer and invokes it.
//  2. JSX-as-call-expression: `const w = <Comp x={1} />` is just an
//     expression returning whatever Comp returns.
//  3. Built-in widget intrinsics (#689): `<Box>` / `<Text>` lower
//     through the codegen rewriter in `crates/perry-codegen/src/lower_call.rs`
//     so the JSX form is interchangeable with the function-call form
//     `Box([...])` / `Text("…")`. Acceptance below covers single-child,
//     multi-child, and styled variants.
//
// Follow-up scope (not covered here):
//  - JSX form for Spacer / Input / Spinner / List / Select / ProgressBar
//    / Table / Tabs / TextArea — the rewriter today only handles
//    Box + Text (#689 v1). Those intrinsics still need the function-call
//    form and continue to fall through to TAG_UNDEFINED in JSX form.

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

// 3a. JSX form: bare Box wrapping a bare Text — the simplest shape that
//     used to fall through to TAG_UNDEFINED before #689. The rewriter
//     turns `jsx(Box, { children: jsx(Text, { children: "hello" }) })`
//     into `Box([Text("hello")])` at compile time.
const bareBoxText = <Box><Text>hello</Text></Box>;
render(bareBoxText);
console.log("\n--- jsx Box+Text ok ---");

// 3b. JSX form: Box with style props + multi-child Text array. Mirrors
//     ink's typical row layout. The rewriter pops `children` out of the
//     props object before forwarding the remaining keys as the style
//     opts arg, and packs the child array as the second positional arg.
const styledRow = (
    <Box flexDirection="row" gap={2}>
        <Text color="red">red</Text>
        <Text color="green">green</Text>
        <Text color="blue">blue</Text>
    </Box>
);
render(styledRow);
console.log("\n--- jsx styled row ok ---");

// 3c. JSX form: Text-only call with style options on Text itself. The
//     rewriter routes through the 2-arg `Text(content, opts)` dispatch
//     so `bold` / `color` etc. land on `js_perry_tui_text_styled`.
const styledText = <Text color="yellow" bold={true}>bold yellow</Text>;
render(Box([styledText]));
console.log("\n--- jsx styled Text ok ---");

/*
@covers
crates/perry-runtime/src/jsx.rs:
  - js_jsx
  - js_jsxs
crates/perry-codegen/src/lower_call.rs:
  - try_rewrite_perry_tui_jsx_intrinsic
  - rewrite_jsx_box
  - rewrite_jsx_text
crates/perry-hir/src/jsx.rs:
  - lower_jsx_element_name (native-module sentinel branch)
*/
