//! A generic class's specialization (`Box$num`, monomorph) has its origin's
//! prototype object at run time (`js_register_class_generic_origin`): one
//! `Box.prototype`, the [[Prototype]] of every specialization's instance. A
//! read of an accessor there runs the origin's getter or setter, what the
//! program installs there at run time is found there, and a method read is
//! the origin's function; a method call runs the specialization's body by
//! class id. SOUND3/SOUND4: test_gap_11183_generic_private_brand,
//! test_edge_complex_patterns, test_gap_specialized_class_frame_identity.
use super::*;

fn reach(src: &str) -> (perry_hir::Module, crate::ProgramFacts) {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    perry_hir::monomorphize_module(&mut hir);
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
    assert!(pf.body_reach.complete, "activated");
    (hir, pf)
}

fn active(pf: &crate::ProgramFacts, id: u32) -> bool {
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

/// Is the accessor `key` (`setter` or getter) of class `class` active?
fn runs(src: &str, class: &str, key: &str, setter: bool) -> bool {
    let (hir, pf) = reach(src);
    let c = hir.classes.iter().find(|c| c.name == class).expect("class");
    let list = if setter { &c.setters } else { &c.getters };
    let id = list.iter().find(|(k, _)| k == key).expect("accessor").1.id;
    active(&pf, id)
}

/// Is method `name` of class `class` active?
fn method_runs(src: &str, class: &str, name: &str) -> bool {
    let (hir, pf) = reach(src);
    let c = hir.classes.iter().find(|c| c.name == class).expect("class");
    let id = c.methods.iter().find(|m| m.name == name).expect("method").id;
    active(&pf, id)
}

#[test]
fn a_specialization_runs_its_origin_accessor() {
    let get = r#"class Box<T> {
  v: T | undefined = undefined;
  get value() { return this.v; }
}
const b = new Box<number>();
console.log(b.value);"#;
    let set = r#"class Box<T> {
  v: T | undefined = undefined;
  set value(x: T | undefined) { this.v = x; }
}
const b = new Box<number>();
b.value = 3;
console.log(b.v);"#;
    assert!(runs(get, "Box", "value", false), "origin getter inactive");
    assert!(runs(set, "Box", "value", true), "origin setter inactive");
}

/// Control: an accessor of a generic class nothing reads stays inactive, on
/// the origin and on the specialization.
#[test]
fn an_unread_specialization_accessor_stays_inactive() {
    let src = r#"class Box<T> {
  v: T | undefined = undefined;
  get value() { return this.v; }
  get other() { return 1; }
}
const b = new Box<number>();
console.log(b.value);"#;
    assert!(runs(src, "Box", "value", false), "read getter inactive");
    assert!(
        !runs(src, "Box", "other", false),
        "unread origin getter active"
    );
    assert!(
        !runs(src, "Box$num", "other", false),
        "unread getter active"
    );
}

/// An accessor the program installs on the generic class's prototype at run
/// time is on the specialization's instances' prototype: the same object.
#[test]
fn an_accessor_defined_on_the_generic_prototype_runs_for_a_specialization() {
    let src = r#"class Box<T> {
  v: T;
  constructor(v: T) { this.v = v; }
}
function wget(this: any) { return 7; }
Object.defineProperty(Box.prototype, "w", { get: wget, configurable: true });
const b = new Box<number>(1);
console.log((b as any).w);"#;
    let (hir, pf) = reach(src);
    let id = hir.functions.iter().find(|f| f.name == "wget").expect("wget").id;
    assert!(active(&pf, id), "getter defined on Box.prototype inactive");
}

/// A method read on a specialization's instance is the prototype's value,
/// the origin's function; a method call runs the specialization's body.
#[test]
fn a_method_read_is_the_origin_function_a_call_the_specialized_body() {
    let read = r#"class Box<T> {
  v: T;
  constructor(v: T) { this.v = v; }
  foo(): string { return "o " + typeof this.v; }
}
const b = new Box<number>(1);
const f = b.foo;
console.log(f.call(b));"#;
    let call = r#"class Box<T> {
  v: T;
  constructor(v: T) { this.v = v; }
  foo(): string { return "o " + typeof this.v; }
}
const b = new Box<number>(1);
console.log(b.foo());"#;
    assert!(method_runs(read, "Box", "foo"), "origin method read inactive");
    assert!(method_runs(call, "Box$num", "foo"), "specialized method inactive");
}
