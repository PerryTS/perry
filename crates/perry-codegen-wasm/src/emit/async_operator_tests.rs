//! Operators inside an async body on the web target, which is emitted as JavaScript.

use super::run_web_source;

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
