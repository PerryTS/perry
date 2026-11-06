//! A builtin method read through its home object (`Array.prototype.slice`,
//! `Object.prototype.hasOwnProperty`) is the native it names, and
//! `call`/`apply`/`bind` on it run that native's model with the receiver
//! given, as a call of the member by name on that receiver does. A value
//! that may be the program's (a shadowed or reassigned namespace, a
//! variable), a key the prototype does not hold as its own final method,
//! or a receiver of another family keeps the unknown callee.

use super::*;

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// The verdict of the one `sv` read (in `g`).
fn sv(body: &str) -> Verdict {
    let src = format!(
        r#"
        class Sym {{ sv: number; constructor(sv: number) {{ this.sv = sv; }} }}
        function g(o: any) {{ return o.sv; }}
        {body}
        "#
    );
    site(&facts(&src), "sv").verdict.clone()
}

/// `sv`, lowered as the driver lowers a program that stores into the global
/// object under a namespace's name (DEC 255, `scan_patched_builtins`).
fn sv_patched(body: &str) -> Verdict {
    let src = format!(
        r#"
        class Sym {{ sv: number; constructor(sv: number) {{ this.sv = sv; }} }}
        function g(o: any) {{ return o.sv; }}
        {body}
        "#
    );
    let ast = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let mut pb = perry_hir::patched_builtins::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(&ast, &mut pb);
    perry_hir::patched_builtins::set_patched_builtins(std::sync::Arc::new(pb));
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_hir::patched_builtins::clear_patched_builtins();
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    let pf = compute(&[("t.ts".to_string(), &hir)], &cfg);
    site(&pf, "sv").verdict.clone()
}

/// node: prints `1`. `slice` runs on the array: its element is the `Sym`.
#[test]
fn array_prototype_slice_call_runs_its_model() {
    let v = sv(r#"
        const arr = [new Sym(1)];
        const out = Array.prototype.slice.call(arr);
        console.log(g(out[0]));
    "#);
    assert!(proven(&v), "{v:?}");
}

/// node: prints `1`. The value kept in a binding, called with `apply`.
#[test]
fn array_prototype_slice_value_apply_runs_its_model() {
    let v = sv(r#"
        const slice = Array.prototype.slice;
        const arr = [new Sym(1)];
        const out = slice.apply(arr, [0]);
        console.log(g(out[0]));
    "#);
    assert!(proven(&v), "{v:?}");
}

/// node: prints `1`. `hasOwnProperty` of `Object.prototype`, bound to the
/// map: an own-key test that reads nothing else; the map keeps its field.
#[test]
fn object_prototype_has_own_property_bind_reads_only() {
    let v = sv(r#"
        const has = Object.prototype.hasOwnProperty;
        const map: any = {};
        map.a = new Sym(1);
        const own = has.bind(map);
        if (own("a")) console.log(g(map.a));
    "#);
    assert!(proven(&v), "{v:?}");
}

/// Hands `this` to an unseen module (in the programs that load one).
const HOOK: &str = "function (this: any) { (globalThis as any).hook = this; return [this[0]]; }";

/// node (`slice` of the program's object): prints `1`. `Array` is a local
/// binding: its `prototype.slice` is the program's function, which hands
/// the array to code the analysis cannot see.
#[test]
fn a_shadowed_namespace_stays_unknown() {
    let v = sv(&unseen(&format!(
        r#"
        function main() {{
            const Array: any = {{ prototype: {{ slice: {HOOK} }} }};
            const arr = [new Sym(1)];
            Array.prototype.slice.call(arr);
            return g(arr[0]);
        }}
        console.log(main());
    "#
    )));
    assert!(!proven(&v), "{v:?}");
}

/// node: prints `1`. The global binding `Array` reassigned through
/// `globalThis`: the read is of the program's object.
#[test]
fn a_reassigned_namespace_stays_unknown() {
    let v = sv_patched(&format!(
        r#"
        (globalThis as any).Array = {{ prototype: {{ slice: {HOOK} }} }};
        const arr = [new Sym(1)];
        Array.prototype.slice.call(arr);
        console.log(g(arr[0]));
    "#
    ));
    assert!(!proven(&v), "{v:?}");
}

/// node: prints `1`. A variable that may hold the program's object.
#[test]
fn a_variable_that_may_hold_a_program_object_stays_unknown() {
    let v = sv(&unseen(&format!(
        r#"
        const A: any = process.argv.length > 99 ? Array : {{ prototype: {{ slice: {HOOK} }} }};
        const arr = [new Sym(1)];
        A.prototype.slice.call(arr);
        console.log(g(arr[0]));
    "#
    )));
    assert!(!proven(&v), "{v:?}");
}

/// node: prints `1`. A key `Array.prototype` does not declare: the program
/// added it (an added key is no realm member), so the read is its function.
#[test]
fn an_added_key_stays_unknown() {
    let v = sv(&format!(
        r#"
        (Array.prototype as any).grab = {HOOK};
        const arr = [new Sym(1)];
        (Array.prototype as any).grab.call(arr);
        console.log(g(arr[0]));
    "#
    ));
    assert!(!proven(&v), "{v:?}");
}

/// node: prints `1`. `Map.prototype.get` called on an array throws: no model
/// of it runs on the array, which escapes as before.
#[test]
fn a_member_of_another_family_stays_unknown() {
    let v = sv(r#"
        const arr = [new Sym(1)];
        try { Map.prototype.get.call(arr, 0); } catch (e) {}
        console.log(g(arr[0]));
    "#);
    assert!(!proven(&v), "{v:?}");
}

/// node: prints `1`. A foreign receiver of `slice`, a plain array-like:
/// the model is the array's, so the object keeps today's unknown call.
#[test]
fn slice_on_an_array_like_stays_unknown() {
    let v = sv(r#"
        const like: any = { length: 1, 0: new Sym(1) };
        const out = Array.prototype.slice.call(like);
        console.log(g(out[0]));
    "#);
    assert!(!proven(&v), "{v:?}");
}
