//! Rule G1 (bool_field_guard): where `x.f === false` (`=== true`, `!==`)
//! is decided, `x` holds only the values whose read of `f` can decide it
//! that way (ECMA-262 7.2.16 IsStrictlyEqual, 14.6 `if`, 14.10 `return`,
//! 13.13 `&&`). `x` is a local, a parameter, or `this` of a function with
//! its own `this` binding. three.js `Cache.get`: `if (this.enabled ===
//! false) return;` on `Cache = { enabled: false, ... }` never reads
//! `this.files[key]`, whose unknown key would give `Object.prototype`
//! methods.
use super::*;
use crate::model::prim;

const CACHE: &str = "const Cache: any = { enabled: false, files: {},
    get(key: any) { if (this.enabled === false) return; return this.files[key]; } };";

fn src(cache: &str, extra: &str, call: &str) -> String {
    format!(
        "{cache}
         class Box {{ v: any; constructor(v: any) {{ this.v = v; }} }}
         {extra}
         const b = new Box({call});
         console.log(b.v);"
    )
}

/// What `b.v` (the result) holds: a function or object (an
/// `Object.prototype` method) or ⊤, and its primitive kinds.
fn result(pf: &ProgramFacts) -> (bool, u16) {
    let k = &field(tmpl(pf, "Box"), "v").kind;
    (
        k.top || k.funcs > 0 || k.builtin_objs > 0 || !k.objs.is_empty(),
        k.prims,
    )
}

fn closed(s: &str) {
    let pf = facts(s);
    let (methods, prims) = result(&pf);
    assert!(
        !methods && prims & !(prim::UNDEF | prim::FALSE) == 0,
        "methods {methods} prims {}\n{}",
        prim::name(prims),
        pf.dump_text()
    );
}

fn open(s: &str) {
    let (methods, _) = result(&facts(s));
    assert!(methods, "the result must keep the methods");
}

const CALL: &str = "Cache.get(process.argv[2])";

#[test]
fn this_false_flag_skips_the_rest_of_the_method() {
    closed(&src(CACHE, "", CALL));
}

#[test]
fn not_false_test_true_branch_is_dead() {
    let c = CACHE.replace(
        "if (this.enabled === false) return; return this.files[key];",
        "if (this.enabled !== false) { return this.files[key]; } return undefined;",
    );
    closed(&src(&c, "", CALL));
}

#[test]
fn and_operand_is_dead() {
    let c = CACHE.replace(
        "if (this.enabled === false) return; return this.files[key];",
        "return this.enabled !== false && this.files[key];",
    );
    closed(&src(&c, "", CALL));
}

#[test]
fn true_test_on_a_false_flag_is_dead() {
    let c = CACHE.replace(
        "if (this.enabled === false) return; return this.files[key];",
        "if (this.enabled === true) { return this.files[key]; } return undefined;",
    );
    closed(&src(&c, "", CALL));
}

#[test]
fn parameter_host() {
    let c = "const Cache: any = { enabled: false, files: {} };
        function get(c: any, key: any) { if (c.enabled === false) return; return c.files[key]; }";
    closed(&src(c, "", "get(Cache, process.argv[2])"));
}

#[test]
fn const_local_host() {
    let c = "const Cache: any = { enabled: false, files: {} };
        function get(key: any) { const c = Cache; if (c.enabled === false) throw 0; return c.files[key]; }";
    closed(&src(c, "", "get(process.argv[2])"));
}

#[test]
fn true_store_keeps_it_open() {
    open(&src(CACHE, "Cache.enabled = true;", CALL));
}

#[test]
fn unknown_key_store_keeps_it_open() {
    open(&src(CACHE, "Cache[process.argv[3]] = 1;", CALL));
}

#[test]
fn unknown_code_keeps_it_open() {
    open(&src(CACHE, "new Function(process.argv[2])(Cache);", CALL));
}

#[test]
fn getter_flag_keeps_it_open() {
    // A getter whose result is not one constant (a getter returning the
    // literal `false` decides the test like the data field).
    let c = CACHE.replace(
        "enabled: false,",
        "get enabled() { return process.argv.length > 5; },",
    );
    open(&src(&c, "", CALL));
}

#[test]
fn another_receiver_keeps_it_open() {
    open(&src(
        CACHE,
        "",
        "Cache.get.call({ enabled: true, files: {} }, process.argv[2])",
    ));
}

#[test]
fn arrow_this_is_not_a_host() {
    let c = CACHE.replace(
        "if (this.enabled === false) return; return this.files[key];",
        "const f = () => { if (this.enabled === false) return; return this.files[key]; }; return f();",
    );
    open(&src(&c, "", CALL));
}
