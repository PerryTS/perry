//! Regression tests for #806: a class with NO own constructor forwarding
//! `new`-site args to an inherited constructor that carries synthesized
//! `__perry_cap_*` params.
//!
//! The no-own-ctor walk in `lower_call/new.rs` forwarded the `new` SITE's
//! `caps_absent_from_args` flag into the ancestor's `CaptureFill`, so the
//! binder derived the user/cap tail-split from the ANCESTOR's cap params.
//! But the site appends the LEAF's captures — and a capturing leaf always
//! has a (synthesized) own ctor, so any leaf that reaches the walk appended
//! nothing. The mis-derived split ate trailing USER args into cap slots:
//! `class Wrapped extends WithSuffix(Logged) {}` + `new Wrapped("alpha")`
//! bound the mixin ctor's `seed` param to `undefined` (the decl-site
//! snapshot silently rescued the cap itself, so only the user arg was
//! lost). The ancestor fill is now unconditionally caps-absent.
//!
//! All expected outputs are byte-for-byte what `node
//! --experimental-strip-types` prints.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The #806 fixture shape: a top-level (capture-free, ctor-free) class
/// extending a capturing mixin class. The default derived ctor must
/// forward the user arg to the mixin ctor's USER param, not its cap slot.
///
/// Still failing — a DIFFERENT root than the walk: the parent here is a
/// per-evaluation class OBJECT (extends-expr), so the leaf's codegen
/// default-derived super forwards through `js_fetch_or_value_super` →
/// `js_native_call_value` → the heap-class-object construct dispatch,
/// whose `user_params = total − max(ctor_caps, snapshot)` subtraction
/// assumes cap params ride in the ctor SIGNATURE. The mixin's compiled
/// ctor is capless (`(this, seed)` — its parent is fetched via the
/// dynamic-parent registry, not a cap param) while a decl-site snapshot
/// IS registered, so user_params computes to 0 and `seed` never receives
/// "alpha". Needs the signature cap-param count registered alongside the
/// ctor (see the follow-up to #806).
#[ignore = "part 2: capless-signature ctor vs snapshot subtraction in the class-object construct dispatch"]
#[test]
fn default_derived_forwards_through_capturing_mixin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
class Logged {
  log: string;

  constructor(seed: string) {
    this.log = "seed=" + seed;
  }
}

type Ctor<T = {}> = new (...args: any[]) => T;

function WithSuffix<TBase extends Ctor<Logged>>(B: TBase) {
  return class extends B {
    constructor(seed: string) {
      super(seed);
      this.log += ":wrapped";
    }
  };
}

class WrappedLogged extends WithSuffix(Logged) {}

console.log("super-args.log:", new WrappedLogged("alpha").log);
"#,
    );
    assert_eq!(out, "super-args.log: seed=alpha:wrapped\n");
}

/// In-scope bare-ident `new` of a ctor-free subclass whose parent ctor
/// captures an enclosing local: the walk binds the parent ctor (cap param
/// `__perry_cap_env`), and the lone user arg must reach the parent's user
/// param while the cap fills from the decl-site snapshot.
#[test]
fn ctor_free_subclass_of_capturing_parent_in_scope() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
function make(env: string): string {
  class P {
    v: string;
    constructor(x: string) {
      this.v = env + ":" + x;
    }
  }
  class C extends P {}
  return new C("u").v;
}

console.log(make("E"));
"#,
    );
    assert_eq!(out, "E:u\n");
}

/// Two ctor-free hops above the capturing ctor, and more than one user
/// arg — every user arg must survive the walk.
#[test]
fn two_hop_walk_multiple_user_args() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
function build(tag: string): string {
  class Base {
    s: string;
    constructor(a: string, b: string) {
      this.s = tag + "/" + a + "/" + b;
    }
  }
  class Mid extends Base {}
  class Leaf extends Mid {}
  return new Leaf("one", "two").s;
}

console.log(build("T"));
"#,
    );
    assert_eq!(out, "T/one/two\n");
}

/// Guard the CAPTURING-leaf path (zod chains): a capturing subclass gets a
/// synthesized forwarding ctor and takes the own-ctor path, whose tail-split
/// derives from its own appended caps. Both the subclass's snapshot and the
/// forwarded user arg must land.
#[test]
fn capturing_subclass_still_forwards_via_synthesized_ctor() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r##"
function factory(kind: string): string {
  class Type {
    def: string;
    constructor(def: string) {
      this.def = def;
    }
    describe(): string {
      return kind + "(" + this.def + ")";
    }
  }
  class Num extends Type {
    label(): string {
      return kind + "#" + this.def;
    }
  }
  const n = new Num("d1");
  return n.describe() + " " + n.label();
}

console.log(factory("K"));
"##,
    );
    assert_eq!(out, "K(d1) K#d1\n");
}

/// Rest-param ancestor ctor below a ctor-free leaf: ALL user args must
/// reach the rest array (none eaten by the cap slot, none truncated).
///
/// Still failing — a DIFFERENT root than the walk: `Drain` unions the
/// parent's captures (`synthesize_class_captures` inherits them), so it
/// gets a SYNTHESIZED forwarding ctor and takes the own-ctor path. That
/// synthesis (`class_captures.rs` "spec default ctor" arm) does NOT
/// synthesize `constructor(...args){ super(...args) }` — it walks the
/// nearest ancestor ctor's user arity and mints that many FIXED params
/// (`__perry_dflt_arg_i`, `is_rest: false`). A rest-param ancestor counts
/// as arity 1, so `new Drain("a","b","c")` binds only "a" and drops the
/// rest → `P[a]`. (An extends-EXPR parent walks to arity 0 and drops ALL
/// args — the mixin test above hits that variant too, beneath its
/// dispatch-layer failure.) Fix is the spec `(...args)` synthesis via
/// `SuperCallSpread` (see the follow-up to #806).
#[ignore = "part 2: HIR default-ctor synthesis mints fixed arity instead of (...args) — rest ancestors truncate"]
#[test]
fn walk_into_rest_param_capturing_ctor() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
function collect(prefix: string): string {
  class Sink {
    all: string;
    constructor(...parts: string[]) {
      this.all = prefix + "[" + parts.join(",") + "]";
    }
  }
  class Drain extends Sink {}
  return new Drain("a", "b", "c").all;
}

console.log(collect("P"));
"#,
    );
    assert_eq!(out, "P[a,b,c]\n");
}
