//! Class definition evaluation on the web target (`emit/expr/class_eval.rs`).

use super::run_web_source;

#[test]
fn a_class_evaluation_installs_its_computed_members() {
    assert_eq!(
        run_web_source(
            r#"
function make(tag: string) {
  const log: string[] = [];
  class K {
    [(log.push("m"), "m")]() { return "m:" + tag; }
    get [(log.push("g"), "g")]() { return "g:" + tag; }
    set [(log.push("s"), "s")](v: string) { log.push("set:" + v); }
    static [(log.push("sm"), "sm")]() { return "sm:" + tag; }
    static get [(log.push("sg"), "sg")]() { return "sg:" + tag; }
  }
  const o: any = new K();
  o.s = tag;
  console.log(log.join(","), o.m(), o.g, (K as any).sm(), (K as any).sg);
  return K;
}
make("a");
make("b");
"#
        ),
        "m,g,s,sm,sg,set:a m:a g:a sm:a sg:a\nm,g,s,sm,sg,set:b m:b g:b sm:b sg:b"
    );
}

#[test]
fn a_class_evaluation_runs_its_static_initializers_in_order_on_fresh_statics() {
    assert_eq!(
        run_web_source(
            r#"
function make(v: number) {
  const log: string[] = [];
  class K {
    static a = (log.push("a"), v);
    static { log.push("block:" + this.a); }
    static b = (log.push("b"), (this as any).a + 1);
    static [(log.push("key"), "c")] = (log.push("c"), v * 10);
  }
  console.log(log.join(","), (K as any).a, (K as any).b, (K as any).c);
  return K;
}
const k1: any = make(1);
const k2: any = make(2);
console.log(k1 === k2, k1.a, k2.a, k1.c, k2.c);
"#
        ),
        "key,a,block:1,b,c 1 2 10\nkey,a,block:2,b,c 2 3 20\nfalse 1 2 10 20"
    );
}

#[test]
fn a_computed_field_key_is_evaluated_once_per_class_evaluation() {
    assert_eq!(
        run_web_source(
            r#"
function make(p: string) {
  let count = 0;
  const key = "f" + p;
  class K { [(count++, key)] = p; }
  new K(); new K();
  const o: any = new K();
  return [count, o[key], Object.prototype.hasOwnProperty.call(o, key)].join(" ");
}
console.log(make("x"), "|", make("y"));
"#
        ),
        "1 x true | 1 y true"
    );
}

#[test]
fn a_computed_key_reads_the_evaluations_captured_locals() {
    assert_eq!(
        run_web_source(
            r#"
function make(p: string) {
  const k = "m" + p.length;
  class K {
    [k]() { return p; }
    static [k + "s"]() { return p + "s"; }
    get [k + "g"]() { return p + "g"; }
  }
  const o: any = new K();
  return [o.m1(), (K as any).m1s(), o.m1g].join(",");
}
console.log(make("a"), make("b"));
"#
        ),
        "a,as,ag b,bs,bg"
    );
}

#[test]
fn a_class_declared_once_runs_its_member_keys_and_static_block() {
    assert_eq!(
        run_web_source(
            r#"
function f() { let n = 0; class K { [(n++, "m")]() {} } return n; }
function g() { let got = 0; class K { static x = 5; static { got = this.x; } } return got; }
let n = 0;
class A { static x = (n++, 7); static { console.log("block", this.x, n); } }
console.log(f(), g(), n, A.x);
"#
        ),
        "block 7 1\n1 5 1 7"
    );
}

#[test]
fn a_static_block_runs_in_its_evaluations_environment() {
    assert_eq!(
        run_web_source(
            r#"
function make(tag: string) {
  const seen: string[] = [];
  const read = () => { class K { static { seen.push(tag + ":" + (this as any).name); } } return K; };
  read(); read();
  return seen.join(",");
}
console.log(make("a"), make("b"));
"#
        ),
        "a:K,a:K b:K,b:K"
    );
}

#[test]
fn a_computed_static_method_gets_closure_arguments_as_closures() {
    assert_eq!(
        run_web_source(
            r#"
function make(key: string) {
  class K { static [key](cb: (n: number) => number) { return [1, 2].map(cb).join(","); } }
  return K;
}
const C: any = make("run");
console.log(C.run((n: number) => n * 2));
"#
        ),
        "2,4"
    );
}

#[test]
fn a_declared_static_field_shadows_the_class_values_own_properties() {
    assert_eq!(
        run_web_source(
            r#"
class K { static name = "custom"; }
const C: any = K;
console.log(C.name);
"#
        ),
        "custom"
    );
}
