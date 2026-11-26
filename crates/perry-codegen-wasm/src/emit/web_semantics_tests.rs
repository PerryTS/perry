//! Language results the web target must share with Node and the native targets.

use super::{run_web_modules_with_dom, run_web_source, run_web_source_then, STUB_DOM};

// `a || b`, `a && b` and `a ?? b` evaluate `a` once: the value they return is the
// one they tested, so a call on the left runs once. Expected output is Node's.
#[test]
fn logical_operators_evaluate_the_left_operand_once() {
    assert_eq!(
        run_web_source(
            r#"
let calls = 0;
function next(v: any): any { calls++; return v; }
const results = [next("a") || "x", next("") || "x", next("a") && "y", next(0) && "y", next(null) ?? "z", next(0) ?? "z"];
console.log(JSON.stringify(results), calls);
"#
        ),
        r#"["a","x","y",0,"z",0] 6"#
    );
}

// Math.round takes a tie toward +∞ (2.5 → 3, -2.5 → -2), not to the even neighbour.
#[test]
fn math_round_takes_ties_toward_positive_infinity() {
    assert_eq!(
        run_web_source(
            r#"
function id(v: number): number { return v; }
console.log([0.5, 1.5, 2.5, -0.5, -1.5, -2.5, 2.4, 2.6].map(v => Math.round(id(v))).join(" "));
"#
        ),
        "1 2 3 0 -1 -2 2 3"
    );
}

// `declare const __platform__` reads the target's platform id; the web target is 5.
#[test]
fn platform_constant_is_the_web_id() {
    assert_eq!(
        run_web_source(
            r#"
declare const __platform__: number;
console.log(typeof __platform__, __platform__);
"#
        ),
        "number 5"
    );
}

// A `void` FFI function called as a statement: the call leaves a value for the
// statement to drop, so the module validates, and the host's missing function is
// the bridge's no-op stub.
#[test]
fn a_void_ffi_call_statement_runs() {
    assert_eq!(
        run_web_source(
            r#"
declare function host_tick(): void;
const ready = true;
if (ready) host_tick();
host_tick();
console.log("ran");
"#
        ),
        "ran"
    );
}

// An import's NaN-boxed arguments reach it with their bits: the wrapper never
// collects them in an array of numbers, where V8 may silence a NaN's payload (it
// did in Chrome, turning a string passed to an async function into NaN). The
// array methods here silence NaNs the same way, so a wrapper that used them fails.
#[test]
fn import_arguments_keep_their_nan_boxed_bits() {
    assert_eq!(
        run_web_source_then(
            r#"console.log("booted");"#,
            r#"
const map = Array.prototype.map, from = Array.from;
const quiet = (values) => { for (let i = 0; i < values.length; i++) if (Number.isNaN(values[i])) values[i] = NaN; return values; };
Array.prototype.map = function (...a) { return quiet(map.apply(this, a)); };
Array.from = function (...a) { return quiet(from.apply(this, a)); };
const boxed = 0x7FFF000000002462n;
for (const count of [1, 2, 4, 6]) {
  let got = [];
  const wrapped = wrapForI64(function (...xs) { got = xs; return 0; });
  wrapped(...new Array(count).fill(boxed));
  console.log(count + " " + got.every(x => f64ToU64(x) === boxed));
}
"#
        ),
        "booted\n1 true\n2 true\n4 true\n6 true"
    );
}

// The same call to a `void` FFI function another module declares and exports, as a
// native library package does (`@perryts/storekit`).
#[test]
fn an_imported_void_ffi_call_statement_runs() {
    assert_eq!(
        run_web_modules_with_dom(
            &[
                ("native", "export declare function host_tick(): void;"),
                (
                    "main",
                    r#"
import { host_tick } from "./native";
const ready = true;
if (ready) host_tick();
host_tick();
console.log("ran");
"#,
                ),
            ],
            STUB_DOM,
            "",
        ),
        "ran"
    );
}
