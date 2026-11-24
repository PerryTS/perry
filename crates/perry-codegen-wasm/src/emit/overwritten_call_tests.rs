//! The runtime's own calls on the web target while a program has overwritten
//! `Function.prototype.call` and `Function.prototype.apply`.

use super::run_web_source;

/// The output of `body`'s `run()`, called with every `call` / `apply` throwing. The
/// originals are back before it prints, so the host's console is not what is tested.
fn run_overwritten(body: &str) -> String {
    run_web_source(&format!(
        r#"
const fp: any = Function.prototype;
const call = fp.call;
const apply = fp.apply;
const boom = () => {{ throw new Error("program call/apply reached"); }};
{body}
let out = "";
fp.call = boom;
fp.apply = boom;
try {{ out = run(); }} catch (e: any) {{ out = "threw " + e.message; }}
fp.call = call;
fp.apply = apply;
console.log(out);
"#
    ))
}

// Expected output is Node's.
#[test]
fn class_members_ignore_an_overwritten_call_and_apply() {
    assert_eq!(
        run_overwritten(
            r#"
const k = "ck";
class A { static s = 1; static sm() { return "sm"; } v = 2; [k] = 3; get g() { return this.v; } set g(x: number) { this.v = x; } m() { return "m"; } }
class B extends A { m() { return super.m() + "b"; } }
class D extends Date { constructor(t: number) { super(t); } }
function make(tag: string) { return class { who() { return tag; } }; }
function run() {
  const b = new B();
  b.g = 7;
  const E = make("e");
  (A as any).s = 5;
  const f = b.m;
  return [b.m(), typeof f, b.g, (b as any)[k], A.s, B.sm(), new E().who(), b instanceof A, new D(9).getTime(), "m" in b].join(",");
}
"#
        ),
        "mb,function,7,3,5,sm,e,true,9,true"
    );
}

// Expected output is Node's.
#[test]
fn iteration_and_descriptors_ignore_an_overwritten_call_and_apply() {
    assert_eq!(
        run_overwritten(
            r#"
class S { static f = 4; }
const src = new Set([1, 2, 3]);
const m = new Map([["k", 1]]);
function run() {
  const out: any[] = [];
  for (const x of src) { out.push(x); if (x === 2) break; }
  const [p, ...rest] = src as any;
  const [q] = [9, 8];
  const o: any = {};
  Object.defineProperties(o, { a: { value: 1 }, b: { get() { return 2; } } });
  const d: any = Object.getOwnPropertyDescriptor(S, "f");
  return [out.join(), p, rest.join(), q, [...(src as any), ...m.keys()].join(), o.a, o.b, d.value].join(";");
}
"#
        ),
        "1,2;1;2,3;9;1,2,3,k;1;2;4"
    );
}

// Expected output is Node's.
#[test]
fn host_method_bridges_ignore_an_overwritten_call_and_apply() {
    assert_eq!(
        run_overwritten(
            r#"
const a = [3, 1, 2];
const d = new Date(0);
const w = new WeakRef(a);
function run() {
  d.setTime(5);
  d.setUTCFullYear(2001);
  return [a.findLast((x) => x < 3), a.findLastIndex((x) => x === 1), a.reduceRight((p, c) => p + "" + c, ""), a.toReversed().join(""), a.toSorted().join(""), a.toSorted((p, q) => q - p).join(""), a.toSpliced(0, 1, 9).join(""), a.with(0, 7).join(""), [1, 2, 3].copyWithin(0, 1).join(""), d.getTime(), d.toUTCString(), w.deref() === a].join(";");
}
"#
        ),
        "2;1;213;213;123;321;912;712;233;978307200005;Mon, 01 Jan 2001 00:00:00 GMT;true"
    );
}
