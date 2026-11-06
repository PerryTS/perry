// Regression test for #679 Phase 1: ink-API ergonomics hooks.
//
// Verifies that useState/useEffect/useApp/useStdout/useMemo/useRef
// compile and dispatch to the runtime FFI. The full counter program
// from the acceptance test requires JSX + destructuring (deferred);
// this test exercises the hooks in their function-call form which is
// what the runtime actually sees after lowering.
//
// We don't enter the run loop — that requires a TTY. Instead we
// invoke the hooks directly and assert their return values via
// console.log + grep on the parity runner.

import {
    useState,
    useStateSet,
    useApp,
    useStdout,
    useRef,
    useMemo,
    useEffect,
} from "perry/tui";

// useState — first call initialises to 5, returns 5.
const v0 = useState(5);
console.log("useState_first=" + v0);

// Slot 0 — write 7 via useStateSet, then read it back. Note: useState
// advances the hook index, so a second `useState(0)` would read a
// fresh slot 1, not slot 0. We use the explicit setter directly.
useStateSet(0, 7);
// We cannot re-read slot 0 via useState without resetting the hook
// index (which only run() does). Outside a run loop, hooks are linear.

// useRef — same machinery, but doesn't trigger STATE_DIRTY.
const ref = useRef(42);
const refVal0 = ref.get();
console.log("useRef_initial=" + refVal0);
ref.set(99);
const refVal1 = ref.get();
console.log("useRef_updated=" + refVal1);

// useApp — singleton handle with exit() / waitUntilExit().
const app = useApp();
// We can't call app.exit() here because it'd flip the global flag
// and any subsequent test relying on the un-exited state would fail.
// Type-check that the handle exists.
console.log("useApp_ok=" + (app !== undefined));

// useStdout — singleton handle with write() / columns() / rows().
const stdout = useStdout();
const cols = stdout.columns();
const rows = stdout.rows();
// We don't assert exact dims (parity runner has different terminal
// sizes); just confirm both are positive numbers.
console.log("useStdout_cols_positive=" + (cols >= 1));
console.log("useStdout_rows_positive=" + (rows >= 1));
stdout.write("useStdout_write_ok\n");

// useMemo — runs fn first time; with no deps the same array on
// subsequent calls is treated as "deps changed" (different arrays
// hash differently). We rely on first-call semantics here.
const computed = useMemo(() => 2 + 3, []);
console.log("useMemo_first=" + computed);

// useEffect — runs the effect immediately on first call. We capture a
// side effect via console.log inside the effect.
useEffect(() => {
    console.log("useEffect_ran=1");
}, []);

console.log("DONE");
