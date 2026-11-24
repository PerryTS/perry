//! Operators inside an async body on the web target, which is emitted as JavaScript.

use super::run_web_source;

// An async body calls a function with no return value, and another async function
// with and without its optional argument. Expected output is Node's.
#[test]
fn async_body_calls_a_void_function_and_an_async_function() {
    assert_eq!(
        run_web_source(
            r#"
function note(label: string, value: unknown) {
  console.log(label, JSON.stringify(value));
}
async function inner(x: number, y?: number): Promise<number> {
  return x + (y ?? 10);
}
async function outer(): Promise<string> {
  note("note", [1, 2]);
  const a = await inner(1);
  const b = await inner(1, 2);
  return a + " " + b;
}
outer().then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "note [1,2]\n11 3"
    );
}

// An async body computes `**`, the bitwise and shift operators (compound assignments
// included), unary `~` / `+`, and `in` as a synchronous one does, and runs `for...in`,
// whose lowering uses `&` and `in`; `in` evaluates its property before its object in
// both bodies. Expected output is Node's.
#[test]
fn async_body_operators_and_for_in_match_sync_code() {
    assert_eq!(
        run_web_source(
            r#"
async function ops(a: number, b: number, u: any, o: any): Promise<string> {
  let k = "a";
  let x = 12; x &= 10; let y = 1; y <<= 3; let z = 5; z |= 2; let w = 6; w ^= 3;
  return [a & b, a | b, a ^ b, a << 2, a >> 1, -a >>> 28, ~a, 2 ** 10, +u, u & 7, x, y, z, w].join(",")
    + " " + ["a" in o, "z" in o, 0 in [5], 1 in [5], k in (k = "b", { a: 1 })].join(",");
}
async function keys(): Promise<string> {
  const o: any = { a: 1, b: 2, c: 3 }; const ks: string[] = [];
  for (const k in o) ks.push(k + "=" + o[k]);
  for (const k in [7, 8]) ks.push(k);
  for (const k in "xy") ks.push(k);
  for (const k in null as any) ks.push(k);
  return ks.join(",");
}
ops(12, 10, "13", { a: 1 }).then((v) => console.log(v), (e) => console.log("threw " + e.message));
keys().then((v) => console.log(v), (e) => console.log("threw " + e.message));
function syncIn(): boolean { let k = "a"; return k in (k = "b", { a: 1 }); }
console.log(syncIn());
"#
        ),
        "true\n8,14,6,48,6,15,-13,1024,13,5,8,8,7,5 true,false,true,false,true\na=1,b=2,c=3,0,1,0,1"
    );
}

// An async body converts the operands of `-`, `*`, `/` and `%` with ToNumber, as a
// synchronous one does. Expected output is Node's.
#[test]
fn async_body_arithmetic_converts_operands_to_numbers() {
    assert_eq!(
        run_web_source(
            r#"
async function f(a: any, b: any): Promise<string> {
  return ["5" - 2, "6" * "2", "7" / "2", "7" % "4", true * 3, null - 1, a - b, a * b, a / b, a % b].join(",");
}
f("9", 4).then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "3,12,3.5,3,3,-1,5,36,2.25,1"
    );
}

// An async body's labeled `continue` reaches the labeled `for` and `while`, through a
// chain of labels too, and a labeled block still takes `break`. Expected output is Node's.
#[test]
fn async_body_labeled_continue_targets_its_loop() {
    assert_eq!(
        run_web_source(
            r#"
async function f(): Promise<string> {
  let r = "";
  outer: for (let i = 0; i < 2; i++) {
    for (let j = 0; j < 2; j++) { if (j === 0) continue outer; r += i + "" + j; }
  }
  let i = 0;
  w: while (i < 3) { i++; for (;;) { if (i === 2) continue w; break; } r += "w" + i; }
  a: b: for (let k = 0; k < 2; k++) { for (;;) { if (k === 0) continue a; break; } r += "a" + k; }
  blk: { r += "b"; if (i === 3) break blk; r += "x"; }
  return r + " done";
}
f().then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "w1w3a1b done"
    );
}

// An async body runs `do...while`, whose body runs once even when the condition starts
// falsy, and whose condition takes JavaScript truthiness. Expected output is Node's.
#[test]
fn async_body_do_while_runs_until_condition_is_falsy() {
    assert_eq!(
        run_web_source(
            r#"
async function f(): Promise<string> {
  let i = 0; do { i++; } while (i < 3);
  let n = 0; do { n++; } while (0);
  let s = "ab"; do { s = s.slice(1); } while (s);
  return i + "," + n + "," + JSON.stringify(s);
}
f().then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "3,1,\"\""
    );
}

// A closure created in an async body is a function that reads its captures, as one
// created in a synchronous body is. Expected output is Node's.
#[test]
fn async_body_closure_is_a_function_over_its_captures() {
    assert_eq!(
        run_web_source(
            r#"
async function f(n: number): Promise<string> {
  const k = 3;
  const g = () => 4;
  const add = (x: number) => x + n + k;
  return [typeof g, g(), add(1), apply(add, 2)].join(",");
}
function apply(fn: (x: number) => number, x: number): number { return fn(x); }
f(10).then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "function,4,14,15"
    );
}

// A local an async body's closure assigns is one binding shared with the body: each
// sees the other's writes, a parameter and a closure that reassigns its own binding included. Expected output is Node's.
#[test]
fn async_body_closure_shares_assigned_captures() {
    assert_eq!(
        run_web_source(
            r#"
async function f(p: number): Promise<string> {
  let count = 0;
  const bump = () => { count += 10; p++; return count; };
  count++;
  const seen = bump();
  count++;
  let caught = "";
  try { throw "e"; } catch (err) { const tag = () => { err += "!"; }; tag(); caught = err; }
  let self = () => { self = () => 7; return 1; };
  return [seen, count, bump(), count, p, caught, self() + self()].join(",");
}
f(1).then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "11,12,22,22,3,e!,8"
    );
}

// A function declaration an async body uses as a value is the same function value a
// synchronous body sees: callable with numbers through a variable, an argument or
// `.map`, an async or rest-parameter function included. Expected output is Node's.
#[test]
fn async_body_function_declaration_is_its_function_value() {
    assert_eq!(
        run_web_source(
            r#"
function add(a: number, b: number) { return a + b; }
function inc(x: number) { return x + 1; }
function sum(...xs: number[]) { return xs.reduce((s, x) => s + x, 0); }
async function dbl(x: number) { return x * 2; }
function twice(f: (x: number) => number, x: number) { return f(f(x)); }
const syncAdd = add;
async function run(): Promise<string> {
  const g = add;
  const s = sum;
  const d = dbl;
  return [g(1, 2), twice(inc, 5), typeof add, [1, 2, 3].map(inc).join("|"), s(1, 2, 3), await d(4),
    g === syncAdd, add === add, Object.getPrototypeOf(d) === Object.getPrototypeOf(async () => {})].join(",");
}
run().then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "3,7,function,2|3|4,6,8,true,true,true"
    );
}

// A builtin method call HIR folds into its own node (`a.map(f)`, `a.some(f)`, ...) runs
// in an async body as it does in a synchronous one. Expected output is Node's.
#[test]
fn async_body_folded_array_methods_call_the_method() {
    assert_eq!(
        run_web_source(
            r#"
async function arrays(): Promise<string> {
  const xs = [3, 1, 2];
  const seen: number[] = [];
  xs.forEach((x) => seen.push(x * 10));
  return [xs.map((x) => x + 1).join("|"), xs.filter((x) => x > 1).join("|"), xs.some((x) => x > 2),
    xs.every((x) => x > 2), xs.find((x) => x < 3), xs.findIndex((x) => x === 2),
    xs.reduce((s, x) => s + x, 0), xs.slice().sort((a, b) => a - b).join("|"), seen.join("|"),
    xs.flatMap((x) => [x, x]).length, xs.indexOf(2), xs.includes(1)].join(",");
}
arrays().then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "4|2|3,3|2,true,false,1,2,6,1|2|3,30|10|20,6,2,true"
    );
}

// A function that throws inside an async body started under a synchronous `try` rejects
// that body, at the call, whether or not it returns a value. Expected output is Node's.
#[test]
fn async_body_stops_at_a_throwing_call_under_a_synchronous_try() {
    assert_eq!(
        run_web_source(
            r#"
function bad(x: number) { if (x) throw new Error("void " + x); }
function badValue(x: number): number { if (x) throw new Error("value " + x); return 1; }
function optional(a: number, b?: number) { return a + "/" + b; }
async function caught(): Promise<string> {
  try { bad(1); return "no throw"; } catch (e: any) { return "caught " + e.message; }
}
async function rejected(): Promise<string> { return "no throw " + badValue(2) + optional(1); }
try {
  caught().then((v) => console.log(v));
  rejected().then((v) => console.log(v), (e) => console.log("rejected " + e.message));
  console.log("after");
} catch (e: any) { console.log("sync " + e.message); }
async function calls(): Promise<string> { return optional(1) + " " + optional(1, 2); }
calls().then((v) => console.log(v));
"#
        ),
        "after\ncaught void 1\nrejected value 2\n1/undefined 1/2"
    );
}
