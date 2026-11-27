//! Premise P-PROTOKEY (owner 11-28, the shape of DECISIONS 217): a computed
//! read `C[k]` of a class constructor whose key is an unknown String never
//! reads `C`'s own `prototype` (non-enumerable, non-writable,
//! non-configurable: ECMA-262 15.7.14, 10.2.5). The named forms and a key
//! holding the constant "prototype" read it, and so does a key of
//! reflective enumeration (`Object.getOwnPropertyNames`, `Reflect.ownKeys`,
//! the keys of `Object.getOwnPropertyDescriptors`), which lists
//! non-enumerable keys (SITEB3: three.js `value[u.id]` pulled every class
//! prototype through `instance.constructor[k]`).
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&list, &cfg)
}

/// The kind fact of `Holder.f` after `read` stored what it read into it.
fn holder_kind(read: &str) -> (String, String) {
    let src = format!(
        "class A {{ y = 1; zqm(): number {{ return this.y; }} }}
        class Holder {{ f: any = 0; }}
        const h = new Holder();
        const any_a: any = A;
        {read}
        console.log(h.f, new A().zqm());"
    );
    let pf = facts_of(&src);
    let dump = pf.dump_text();
    let kind = dump
        .lines()
        .find(|l| l.starts_with("FACT F2 kind main.ts:Holder.f = "))
        .unwrap_or("none")
        .to_string();
    (kind, dump)
}

/// May the read give `A.prototype`? The fixture stores `X.zqm` of the
/// value `X` read: only `A.prototype` holds the method `zqm` (A's other own
/// fields are `name`, `length` and what `Function.prototype` gives, none
/// with a `zqm`).
fn holds_prototype(read: &str) -> bool {
    let (kind, dump) = holder_kind(read);
    assert!(kind != "none", "no kind fact: {dump}");
    eprintln!("PK {read} => {kind}");
    kind.split(" = ").nth(1).unwrap_or("").split('|').any(|k| k.starts_with("fn"))
}

/// (a) RED on base: a key of unknown String reads no `prototype`.
#[test]
fn protokey_unknown_key_skips_prototype() {
    assert!(!holds_prototype("const k = String(Date.now()); h.f = (any_a[k]).zqm;"));
}

/// (b) The named forms read it.
#[test]
fn protokey_named_forms_read_prototype() {
    assert!(holds_prototype("h.f = (any_a[\"prototype\"]).zqm;"));
    assert!(holds_prototype("const k = \"prototype\"; h.f = (any_a[k]).zqm;"));
}

/// (c) A key of `Object.getOwnPropertyNames` (non-enumerable keys
/// included, ECMA-262 20.1.2.10) reads it.
#[test]
fn protokey_own_names_key_reads_prototype() {
    assert!(holds_prototype("for (const k of Object.getOwnPropertyNames(any_a)) h.f = (any_a[k]).zqm;"));
}

/// (d) A key of `Reflect.ownKeys` (28.1.11) reads it.
#[test]
fn protokey_reflect_own_keys_reads_prototype() {
    assert!(holds_prototype("for (const k of Reflect.ownKeys(any_a)) h.f = (any_a[k as any]).zqm;"));
    assert!(holds_prototype("const ks = Reflect.ownKeys(any_a); h.f = (any_a[ks[Number(Date.now() % 3)] as any]).zqm;"));
}

/// (d') A key of the object `Object.getOwnPropertyDescriptors` makes
/// (20.1.2.9: one property per own key) reads it.
#[test]
fn protokey_descriptors_keys_read_prototype() {
    assert!(holds_prototype("for (const k in Object.getOwnPropertyDescriptors(any_a)) h.f = (any_a[k]).zqm;"));
    assert!(holds_prototype("for (const k of Object.keys(Object.getOwnPropertyDescriptors(any_a))) h.f = (any_a[k]).zqm;"));
}

/// (e) RED on base: the three.js shape, an instance's `constructor` read
/// under an unknown key, pulls no prototype.
#[test]
fn protokey_instance_constructor_chain_skips_prototype() {
    assert!(!holds_prototype(
        "const a: any = new A(); const k = String(Date.now()); h.f = (a.constructor[k]).zqm;"
    ));
}

/// (f) RED on b7e6dbdd1d: a reflective key joined with an unknown String
/// is still a reflective key (an unknown String is the narrower of the
/// two: `strs_join`), so it reads `prototype`, in either order.
#[test]
fn protokey_reflective_key_joined_with_unknown_reads_prototype() {
    assert!(holds_prototype(
        "const keys = [...Object.getOwnPropertyNames(any_a), String(Date.now())];
        for (const k of keys) h.f = (any_a[k]).zqm;"
    ));
    assert!(holds_prototype(
        "const keys = [String(Date.now()), ...Object.getOwnPropertyNames(any_a)];
        for (const k of keys) h.f = (any_a[k]).zqm;"
    ));
}

/// (g) A for-of key of `Object.getOwnPropertyNames` is no String
/// constant: a store under it may write every own key (`const_str_key`),
/// so `h.f` takes what the loop stores.
#[test]
fn protokey_reflective_loop_key_store_writes_every_key() {
    let (kind, dump) = holder_kind(
        "for (const k of Object.getOwnPropertyNames(any_a)) (h as any)[k] = () => 1;",
    );
    assert!(kind.contains("fn"), "h.f may hold the closure: {kind}\n{dump}");
}
