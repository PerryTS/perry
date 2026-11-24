//! A function declaration used as a value on the web target (`function_value` in
//! `wasm_runtime.js`).

use super::{run_web_modules, run_web_source};

// A function declaration used as a value is one object, with the `prototype` an
// ordinary function has; `F.prototype = x` stores on it. Expected output is Node's.
#[test]
fn a_function_value_keeps_its_identity_and_has_a_prototype() {
    assert_eq!(
        run_web_source(
            r#"
function F(this: any) { this.v = 1; }
function G() {}
const fp: any = (F as any).prototype;
const before = fp;
(F as any).tag = "t";
(F as any).prototype = { replaced: true };
console.log([typeof fp, fp.constructor === F, Object.keys(fp).length, (F as any) === (F as any), (F as any).tag, (F as any).prototype.replaced, before !== (F as any).prototype, (G as any).prototype !== fp, typeof (() => 1 as any).prototype].join(","));
"#
        ),
        "object,true,0,true,t,true,true,true,undefined"
    );
}

// An object whose prototype chain reaches a function is an object, not a function.
#[test]
fn an_object_inheriting_from_a_function_is_not_a_function() {
    assert_eq!(
        run_web_source(
            r#"
function F() {}
const a: any = Object.create(F);
const b: any = Object.setPrototypeOf({}, F);
console.log([typeof a, typeof b, typeof F].join(","));
"#
        ),
        "object,object,function"
    );
}

// An imported function an async body uses as a value is the same function value a
// synchronous body and its own module see: callable with numbers through a variable
// or an argument, an async function included (#15302). Expected output is Node's.
#[test]
fn an_imported_function_value_in_an_async_body_is_its_function_value() {
    assert_eq!(
        run_web_modules(&[
            (
                "lib",
                r#"
export function add(a: number, b: number) { return a + b; }
export function inc(x: number) { return x + 1; }
export async function dbl(x: number) { return x * 2; }
export function isAdd(f: unknown) { return f === add; }
"#
            ),
            (
                "main",
                r#"
import { add, inc, dbl, isAdd } from "./lib";
function twice(f: (x: number) => number, x: number) { return f(f(x)); }
const syncAdd = add;
async function run(): Promise<string> {
  const g = add;
  const d = dbl;
  return [g(1, 2), twice(inc, 5), typeof add, await d(4), g === syncAdd, isAdd(g)].join(",");
}
run().then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
            ),
        ]),
        "3,7,function,8,true,true"
    );
}
