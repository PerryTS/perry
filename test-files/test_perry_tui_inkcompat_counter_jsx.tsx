// #689 — JSX-form variant of the inkcompat counter test.
//
// Mirrors `test_perry_tui_inkcompat_counter.ts` but uses the JSX form
// `<Box><Text>{...}</Text></Box>` instead of the function-call form
// `Box([Text(...)])`. Exercises the codegen JSX intrinsic rewriter
// (`crates/perry-codegen/src/lower_call.rs::try_rewrite_perry_tui_jsx_intrinsic`)
// end-to-end through the same useState / useInput / run / exit
// pipeline as the original test.
//
// Acceptance: prints `FINAL=N` matching whatever the stdin transcript
// drove. Compared against the function-call counter test to confirm
// JSX-form == function-call-form semantics for Box + Text.

import { Box, Text, useState, useInput, run, exit } from "perry/tui";

let finalValue = 0;

run(() => {
    const [n, setN] = useState(0);
    finalValue = n;
    useInput((s: string) => {
        if (s === "+") {
            setN(finalValue + 1);
            finalValue = finalValue + 1;
        }
        if (s === "-") {
            setN(finalValue - 1);
            finalValue = finalValue - 1;
        }
        if (s === "q") exit();
    });
    // Note: JSX text whitespace is trimmed by `normalize_jsx_text`, so
    // we use string concat for the label to match the function-call
    // form's `Text("count: " + n)` output exactly. The JSX `{...}`
    // expression slot still exercises the rewriter — both single-child
    // and multi-child paths are tested in
    // `test_issue_679_perry_tui_jsx_audit.tsx`.
    return (
        <Box>
            <Text>{"count: " + n}</Text>
        </Box>
    );
});

console.log("FINAL=" + finalValue);
