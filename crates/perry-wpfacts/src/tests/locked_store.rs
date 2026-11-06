//! DEC 250, stores: the runtime refuses to add a key to `%Object.prototype%`
//! and `%Function.prototype%` and to replace a member the image gives a
//! locked intrinsic prototype (`builtin_proto::add_locked`,
//! `locked_violation`; `constructor` is exempt). A store under a computed
//! key whose receiver may be a builtin prototype (the collapsed
//! `ObjectProto` token) is kept on that token: an add-open prototype
//! (`Array.prototype`) takes it. A read on a chain that passes only locked
//! prototypes (`members::locked_read`, `locked_chain_read`) is final and
//! never sees it: the chain value and the builtin answer (`chain_load`) are
//! one decision.
use super::*;

fn one(src: &str) -> ProgramFacts {
    run(src, false)
}

/// With dynamic code naming globals (`new Function`), as in fastify: a
/// static `Object.create(q)` is guarded (`NamespacePatched(Object) ?
/// globalThis.Object.create(q) : ObjectCreate(q)`), so its patched arm
/// reads `create` on the `Object` constructor.
fn run(src: &str, interp: bool) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let mut pb = perry_hir::patched_builtins::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(&module, &mut pb);
    let mut cfg = FactsConfig::default();
    if interp {
        pb.dynamic_code_reach
            .record_global("new Function(...) @ x.ts (byte 1) (names globals)".into());
        cfg.dynamic_code = Some("new Function(...) @ x.ts (names globals)".into());
    }
    perry_hir::patched_builtins::set_patched_builtins(std::sync::Arc::new(pb));
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    perry_hir::patched_builtins::clear_patched_builtins();
    crate::compute(&[(path, &hir)], &cfg)
}

fn verdicts(pf: &ProgramFacts, key: &str) -> Vec<Verdict> {
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == key && !s.is_call).collect();
    s.sort_by_key(|s| s.byte_offset);
    s.iter().map(|s| s.verdict.clone()).collect()
}

fn proven(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| !matches!(v, Verdict::No(_)))
}

fn unproven(v: &[Verdict]) -> bool {
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}

/// Isolate a write onto the collapsed prototype from writes to global
/// bindings: a named key cannot also name the global `Object` binding.
/// The computed-key cases below separately keep constructor and add-open
/// prototype stores observable.
fn program(store: &str, body: &str) -> String {
    format!(
        "function C(this: any) {{ this.c = 1; }}\n\
         const ctor: any = process.argv.length > 3 ? C : {store};\n\
         try {{ ctor.prototype.zz = function (z: any) {{ delete z.x; }}; }} catch (e) {{}}\n\
         function main() {{ {body} }}\nconsole.log(main());"
    )
}

/// The missing member is final on the Object namespace's described
/// Function.prototype -> Object.prototype chain. The base incorrectly
/// replays the store and calls it on q; the control's program prototype
/// is not on this chain.
const MISS: &str = "const q: any = { x: 1 }; const f: any = (Object as any).zz; if (typeof f === 'function') f(q); return q.x;";

#[test]
fn a_store_on_object_prototype_is_not_a_chain_member_of_object() {
    let control = run(&program("C", MISS), false);
    assert!(proven(&verdicts(&control, "x")), "control: {}", control.dump_text());
    let pf = run(&program("Object", MISS), false);
    let v = verdicts(&pf, "x");
    assert!(proven(&v), "{v:?}\n{}", pf.dump_text());
}

/// Soundness: `constructor` is exempt from the lock, so a computed store
/// may replace it: `o.constructor` reads the stored function too, and
/// calling it with `p` deletes `p.x`.
#[test]
fn a_computed_store_may_still_write_constructor() {
    let body = "const o: any = { y: 1 }; const p: any = { x: 1 }; const K: any = o.constructor; \
                K(p); return p.x;";
    let src = |store: &str| {
        format!(
            "function C(this: any) {{ this.c = 1; }}\n\
             const ctor: any = process.argv.length > 3 ? C : {store};\n\
             ctor.prototype[process.argv[2]] = function (z: any) {{ delete z.x; }};\n\
             function main() {{ {body} }}\nconsole.log(main());"
        )
    };
    let c = verdicts(&one(&src("C")), "x");
    assert!(proven(&c), "control: {c:?}");
    let pf = one(&src("Object"));
    let v = verdicts(&pf, "x");
    assert!(unproven(&v), "{v:?}\n{}", pf.dump_text());
}

/// Soundness: `Array.prototype` is add-open (`add_locked` is false): a key
/// the program adds there under a computed key is what an array's method
/// call of an absent key runs.
#[test]
fn a_computed_store_on_an_add_open_prototype_reaches_its_reads() {
    let body = "const a: any = []; const p: any = { x: 1 }; a.zz(p); return p.x;";
    let src = |store: &str| {
        format!(
            "function C(this: any) {{ this.c = 1; }}\n\
             const ctor: any = process.argv.length > 3 ? C : {store};\n\
             ctor.prototype[process.argv[2]] = function (z: any) {{ delete z.x; }};\n\
             function main() {{ {body} }}\nconsole.log(main());"
        )
    };
    let pf = one(&src("Array"));
    let v = verdicts(&pf, "x");
    assert!(unproven(&v), "{v:?}\n{}", pf.dump_text());
}

/// A final Object.prototype chain read does not decide an own static of
/// the global object. This own value may be any program function, including
/// one that deletes q.x; the inherited intrinsic is not called instead.
#[test]
fn an_unknown_own_static_shadows_a_locked_global_chain_member() {
    let src = |holder: &str, store: &str| format!(
        "declare const unknown: any;\nconst receiver: any = {holder};\n{store}\n\
         function main() {{\n\
           const q: any = {{ x: 1 }};\n\
           const f: any = receiver.__defineGetter__;\n\
           try {{ if (typeof f === 'function') f(q); }} catch (e) {{}}\n\
           return q.x;\n\
         }}\nconsole.log(main());"
    );
    let control = one(&src("({ __defineGetter__(_q: any) { return 1; } })", ""));
    assert!(proven(&verdicts(&control, "x")), "control: {}", control.dump_text());
    let subject = one(&src("globalThis", "receiver.__defineGetter__ = unknown.f;"));
    assert!(unproven(&verdicts(&subject, "__defineGetter__")), "the own read is not a final native member");
    assert!(unproven(&verdicts(&subject, "x")), "own static: {}", subject.dump_text());
}
