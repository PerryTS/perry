//! A private accessor (`get #x()`, `set #x(v)`, `static get #x()`) is read
//! and written through `Expr::PrivateGuard` under its dispatch key
//! (`perry_hir::private_names::private_member_key`), not under its source
//! spelling: the class holds it under that key. SOUND3:
//! test_gap_private_accessor_probe, test_gap_class_advanced,
//! test_gap_private_member_target_eval_order, test_issue_5893_private_brand_freshness,
//! test_gap_11791_private_error_texts, test_gap_static_private_fields_stack.
use super::*;

/// Is accessor `key` (`setter` or getter) of class `class` active?
fn runs(src: &str, class: &str, key: &str, setter: bool) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    perry_hir::monomorphize_module(&mut hir);
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
    assert!(pf.body_reach.complete, "activated");
    let c = hir.classes.iter().find(|c| c.name == class).expect("class");
    let list = if setter { &c.setters } else { &c.getters };
    let id = list.iter().find(|(k, _)| k == key).expect("accessor").1.id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

#[test]
fn a_private_accessor_runs_where_its_guarded_access_reads_it() {
    let get = r##"class C {
  #n = 5;
  get #twice() { return this.#n * 2; }
  read() { return this.#twice; }
}
console.log(new C().read());"##;
    assert!(runs(get, "C", "#twice", false), "private getter inactive");
    let set = r##"class C {
  #n = 5;
  set #twice(v: number) { this.#n = v; }
  write(v: number) { this.#twice = v; return this.#n; }
}
console.log(new C().write(3));"##;
    assert!(runs(set, "C", "#twice", true), "private setter inactive");
    let stat = r##"class C {
  static get #sg() { return 2; }
  static both() { return C.#sg; }
}
console.log(C.both());"##;
    assert!(
        runs(stat, "C", "#sg", false),
        "static private getter inactive"
    );
}

/// Controls: an unread private getter stays inactive, and a public read of
/// the same spelling (`o["#twice"]`) is another property: it runs no private
/// getter.
#[test]
fn a_private_accessor_is_not_its_public_spelling() {
    let src = r##"class C {
  #n = 5;
  get #twice() { return this.#n * 2; }
  get #other() { return 1; }
  read() { return this.#twice; }
}
const c: any = new C();
console.log(c.read(), c["#other"]);"##;
    assert!(
        runs(src, "C", "#twice", false),
        "read private getter inactive"
    );
    assert!(
        !runs(src, "C", "#other", false),
        "public spelling ran the private getter"
    );
}
