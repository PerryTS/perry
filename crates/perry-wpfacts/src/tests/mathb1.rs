//! MATHB1: the intrinsic-callee verdict of a builtin namespace call
//! (`IntrinsicCallee`, derive.rs `namespace_callee`). A call `N.m(…)`, the
//! HIR pair `NamespacePatched(N) ? N.m(…) : <intrinsic>` (lowered for every
//! direct namespace call under `namespace_pairs`), is one ordinary member
//! call, and the alias `const c = N.m; c(…)` one value call: the verdict
//! says the callee is exactly the original builtin `N.m`, untouched.
//! PRESENT for programs that never write `N.m`; ABSENT wherever a value
//! other than the original may reach the member: a store through any
//! reference, a writer builtin, a delete, an unknown key on N, a ⊤ store.
use super::*;

/// Lower `src` the way the compiler does (the patched-builtins scan first),
/// every direct namespace call as the pair, then solve.
fn solve(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut set = perry_hir::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(&module, &mut set);
    set.namespace_pairs = true;
    perry_hir::set_patched_builtins(std::sync::Arc::new(set));
    let hir = perry_hir::lower_module(&module, "t", "t.ts");
    perry_hir::clear_patched_builtins();
    let hir = hir.expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    cfg.namespace_pairs = true;
    compute(&[("t.ts".to_string(), &hir)], &cfg)
}

/// The intrinsic-callee lines of the dump (`I <start> .<key> <verdict>`)
/// for `key` (`()` for a value call).
fn ic(src: &str, key: &str) -> Vec<String> {
    let pf = solve(src);
    let d = pf.dump_text();
    let v: Vec<String> = d
        .lines()
        .filter_map(|l| l.strip_prefix("I "))
        .filter_map(|l| {
            let (_, rest) = l.split_once(" .")?;
            let (k, verdict) = rest.split_once(' ')?;
            (k == key).then(|| verdict.to_string())
        })
        .collect();
    eprintln!("MB1 {key}: {v:?}");
    v
}

fn present(src: &str, key: &str, name: &str) {
    let v = ic(src, key);
    let want = format!("exact {name}");
    assert!(
        !v.is_empty() && v.iter().all(|x| *x == want),
        "{key}: {v:?}\n{src}"
    );
}

fn absent(src: &str, key: &str, name: &str) {
    let v = ic(src, key);
    let want = format!("exact {name}");
    assert!(v.iter().all(|x| *x != want), "{key}: {v:?}\n{src}");
}

const X: &str = "const x = Number(process.argv[2]);\nconst f = (y: number) => 42 + y;\n";

fn p(body: &str) -> String {
    format!("{X}{body}\nconsole.log(Math.sin(x), Math.cos(x), f(1));\n")
}

// ---- PRESENT ----

#[test]
fn mathb1_plain_call() {
    present(&p(""), "sin", "Math.sin");
}

#[test]
fn mathb1_alias_call() {
    present(
        &p("const c = Math.sin;\nconsole.log(c(x));"),
        "()",
        "Math.sin",
    );
}

#[test]
fn mathb1_namespace_passed_and_read() {
    let src = p("function g(m: any) { return m.PI }\nconsole.log(g(Math));");
    present(&src, "sin", "Math.sin");
}

#[test]
fn mathb1_global_in_field_unrelated_member() {
    let src = p("const ctx: any = {};\nctx.self = globalThis;\nctx.self.setTimeout(() => {}, 0);");
    present(&src, "sin", "Math.sin");
}

/// RED (MATHB2 3): the store `Math.cos = f` hands `f` to unknown code (E20,
/// builtins/methods.rs `tok_store` -> `sink_node` -> `escape_body`), which
/// holds the realm (`hold_realm`): every realm member then counts as
/// written (`realm_replaced`), `Math` read off the global is any value, and
/// the `Math.sin` site is refused. Fix (next phase): a store on a namespace
/// whose members no realm algorithm reads hands the value to nobody.
#[test]
fn mathb1_other_member_written() {
    let src = p("(Math as any).cos = f;");
    present(&src, "sin", "Math.sin");
    absent(&src, "cos", "Math.cos");
}

// ---- ABSENT (KEEP) ----

#[test]
fn mathb1_keep_direct_store() {
    absent(&p("(Math as any).sin = f;"), "sin", "Math.sin");
}

#[test]
fn mathb1_keep_store_through_alias() {
    absent(&p("const m: any = Math;\nm.sin = f;"), "sin", "Math.sin");
}

#[test]
fn mathb1_keep_store_through_computed_global() {
    absent(
        &p("(globalThis as any)['Ma' + 'th'].sin = f;"),
        "sin",
        "Math.sin",
    );
}

#[test]
fn mathb1_keep_unknown_key_writer() {
    let src = p("function w(o: any, k: string) { o[k] = f }\nw(Math, process.argv[2]);");
    absent(&src, "sin", "Math.sin");
    absent(&src, "cos", "Math.cos");
}

#[test]
fn mathb1_keep_object_assign() {
    absent(&p("Object.assign(Math, { sin: f });"), "sin", "Math.sin");
}

#[test]
fn mathb1_keep_define_property() {
    absent(
        &p("Object.defineProperty(Math, 'sin', { value: f });"),
        "sin",
        "Math.sin",
    );
}

#[test]
fn mathb1_keep_reflect_set() {
    absent(&p("Reflect.set(Math, 'sin', f);"), "sin", "Math.sin");
}

#[test]
fn mathb1_keep_delete() {
    absent(&p("delete (Math as any).sin;"), "sin", "Math.sin");
}

#[test]
fn mathb1_keep_conditional_write() {
    absent(
        &p("if (process.argv[2]) (Math as any).sin = f;"),
        "sin",
        "Math.sin",
    );
}

#[test]
fn mathb1_keep_unknown_receiver_unknown_key() {
    let src = p("const o: any = (globalThis as any)[process.argv[2]];\no[process.argv[3]] = f;");
    absent(&src, "sin", "Math.sin");
}

#[test]
fn mathb1_keep_proxy_store() {
    absent(
        &p("const q: any = new Proxy(Math, {});\nq.sin = f;"),
        "sin",
        "Math.sin",
    );
}

#[test]
fn mathb1_keep_alias_after_write() {
    let src = p("const m: any = Math;\nm.sin = f;\nconst c = Math.sin;\nconsole.log(c(x));");
    absent(&src, "()", "Math.sin");
}

// ---- fixtures of the stopped line (s/me1/fx, s/me6/v) ----

#[test]
fn mathb1_fx_d1() {
    absent(
        r#"function w(o: any, k: string) { o[k] = (x: number) => 42; }
w(Math, process.argv[2]); const f = Math.sin; console.log(f(1));
"#,
        "()",
        "Math.sin",
    );
}

#[test]
fn mathb1_fx_d2() {
    absent(
        r#"const g: any = globalThis; g["Ma" + "th"].sin = (x: number) => 42; const f = Math.sin; console.log(f(1));
"#,
        "()",
        "Math.sin",
    );
}

#[test]
fn mathb1_fx_d3() {
    absent(
        r#"const M: any = Math; M.sin = (x: number) => 42; const f = Math.sin; console.log(f(1));
"#,
        "()",
        "Math.sin",
    );
}

#[test]
fn mathb1_fx_d4() {
    absent(
        r#"function w(o: any, k: string) { o[k] = (x: number) => 42; }
const f = Math.sin; w(Math, process.argv[2]); console.log(f(1), Math.sin(1));
"#,
        "sin",
        "Math.sin",
    );
}

#[test]
fn mathb1_fx_d4_alias() {
    absent(
        r#"function w(o: any, k: string) { o[k] = (x: number) => 42; }
const f = Math.sin; w(Math, process.argv[2]); console.log(f(1), Math.sin(1));
"#,
        "()",
        "Math.sin",
    );
}

#[test]
fn mathb1_fx_d5() {
    absent(
        r#"function w(o:any,k:string){o[k]=(x:number)=>42}
w(Math,process.argv[2]); console.log(Math.sin(1))
"#,
        "sin",
        "Math.sin",
    );
}

#[test]
fn mathb1_fx_g6() {
    present(
        r#"function keep( o: any ) { return o }
keep( globalThis );
class Q { a: number; b: number; constructor() { this.a = 1; this.b = 2; Object.defineProperty( this, "id", { value: 7 } ) } }
const qs: Q[] = []; for ( let i = 0; i < 100; i++ ) qs.push( new Q() ); const o = { z: 3 };
let t = 0; for ( let r = 0; r < 20000; r++ ) for ( let i = 0; i < 100; i++ ) { const q = qs[ i ]; t += q.a * q.b + o.z }
console.log( t, (qs[ 0 ] as any).id );
"#,
        "defineProperty",
        "Object.defineProperty",
    );
}

#[test]
fn mathb1_fx_g6n() {
    present(
        r#"function keep( o: any ) { return o }
class Q { a: number; b: number; constructor() { this.a = 1; this.b = 2; Object.defineProperty( this, "id", { value: 7 } ) } }
const qs: Q[] = []; for ( let i = 0; i < 100; i++ ) qs.push( new Q() ); const o = { z: 3 };
let t = 0; for ( let r = 0; r < 20000; r++ ) for ( let i = 0; i < 100; i++ ) { const q = qs[ i ]; t += q.a * q.b + o.z }
console.log( t, (qs[ 0 ] as any).id );
"#,
        "defineProperty",
        "Object.defineProperty",
    );
}

#[test]
fn mathb1_fx_g7() {
    absent(
        r#"function keep( o: any ) { return o }
keep( globalThis );
const box: any = { r: null }; function pick(): any { return box.r }
class Q { a: number; b: number; constructor() { this.a = 1; this.b = 2; Object.defineProperty( this, "id", { value: 7 } ) } }
const qs: Q[] = []; for ( let i = 0; i < 100; i++ ) qs.push( new Q() );
box.r = Object; const k = process.argv.length > 99 ? "x" : "defineProperty";
const orig = Object.defineProperty; const o = pick(); o[ k ] = function ( t: any, kk: any, d: any ) { t.seen = kk; return t };
const q2: any = new Q(); o[ k ] = orig;
let t = 0; for ( let r = 0; r < 2000; r++ ) for ( let i = 0; i < 100; i++ ) { const q = qs[ i ]; t += q.a * q.b }
console.log( t, (qs[ 0 ] as any).id, q2.id, q2.seen, Object.keys( q2 ).join( "," ), "id" in q2 );
"#,
        "defineProperty",
        "Object.defineProperty",
    );
}

#[test]
fn mathb1_fx_g7c() {
    absent(
        r#"function keep( o: any ) { return o }
keep( globalThis );
const box: any = { r: null }; function pick(): any { return box.r }
class Q { a: number; b: number; constructor() { this.a = 1; this.b = 2; Object.defineProperty( this, "id", { value: 7 } ) } }
const qs: Q[] = []; for ( let i = 0; i < 100; i++ ) qs.push( new Q() );
box.r = Object; const k = process.argv.length > 99 ? "x" : "defineProperty";
const orig = Object.defineProperty; const o = box.r; o[ k ] = function ( t: any, kk: any, d: any ) { t.seen = kk; return t };
const q2: any = new Q(); o[ k ] = orig;
let t = 0; for ( let r = 0; r < 2000; r++ ) for ( let i = 0; i < 100; i++ ) { const q = qs[ i ]; t += q.a * q.b }
console.log( t, (qs[ 0 ] as any).id, q2.id, q2.seen, Object.keys( q2 ).join( "," ), "id" in q2 );
"#,
        "defineProperty",
        "Object.defineProperty",
    );
}

#[test]
fn mathb1_fx_g8() {
    absent(
        r#"function keep( o: any ) { return o }
keep( globalThis );
const box: any = { r: null }; function pick(): any { return box.r }
class Q { a: number; b: number; constructor() { this.a = 1; this.b = 2; Object.defineProperty( this, "id", { value: 7 } ) } }
const qs: Q[] = []; for ( let i = 0; i < 100; i++ ) qs.push( new Q() );
box.r = Object; const orig = Object.defineProperty;
function hack() { box.r.defineProperty = function ( t: any, kk: any, d: any ) { t.seen = kk; return t } }
if ( process.argv[ 2 ] ) hack();
const q2: any = new Q(); box.r.defineProperty = orig;
let t = 0; for ( let r = 0; r < 2000; r++ ) for ( let i = 0; i < 100; i++ ) { const q = qs[ i ]; t += q.a * q.b }
console.log( t, (qs[ 0 ] as any).id, q2.id, q2.seen, Object.keys( q2 ).join( "," ), "id" in q2 );
"#,
        "defineProperty",
        "Object.defineProperty",
    );
}

#[test]
fn mathb1_fx_g10() {
    absent(
        r#"function keep( o: any ) { return o }
keep( globalThis );
const box: any = { r: null }; function pick(): any { return box.r }
class Q { a: number; b: number; constructor() { this.a = 1; this.b = 2; Object.defineProperty( this, "id", { value: 7 } ) } }
const qs: Q[] = []; for ( let i = 0; i < 100; i++ ) qs.push( new Q() );
box.r = Object; const orig = Object.defineProperty;
const k2 = String( process.argv[ 2 ] ) === "sin" ? "defineProperty" : String( process.argv[ 2 ] ) + "q";
box.r[ k2 ] = function ( t: any, kk: any, d: any ) { t.seen = kk; return t };
const q2: any = new Q(); box.r[ k2 ] = orig;
let t = 0; for ( let r = 0; r < 2000; r++ ) for ( let i = 0; i < 100; i++ ) { const q = qs[ i ]; t += q.a * q.b }
console.log( t, (qs[ 0 ] as any).id, q2.id, q2.seen, Object.keys( q2 ).join( "," ), "id" in q2 );
"#,
        "defineProperty",
        "Object.defineProperty",
    );
}

// ---- MATHB1B: the pair keeps the facts of the code around it ----

/// A constructor that defines a key on `this` through the pair: the
/// intrinsic arm is the birth define (ctor_phase Rule BD-A), the generic
/// call binds nothing while `Object.defineProperty` is the original, so the
/// instance is no HARD object and its reads and stores stay proven.
/// RED (MATHB1B): ctor_phase registers the define (birth_defines = [id])
/// but the intrinsic arm still takes the non-birth E5 path
/// (gen_rest/builtin_objects.rs:118 `birth_define_owner`): next phase.
#[test]
fn mathb1b_ctor_define_through_the_pair() {
    let src = "let n = Number(process.argv[2]);\nclass A { x: number; constructor() { Object.defineProperty(this, 'id', { value: n++ }); this.x = Math.sin(n); } }\nconst a = new A();\nconsole.log(a.x, (a as any).id);\n";
    let pf = solve(src);
    let d = pf.dump_text();
    let bad: Vec<&str> = d
        .lines()
        .filter(|l| l.starts_with("S ") && l.contains("verdict=no"))
        .collect();
    eprintln!("MB1B {bad:?}");
    assert!(bad.is_empty(), "{bad:?}");
    assert!(ic(src, "defineProperty")
        .iter()
        .all(|v| v == "exact Object.defineProperty"));
}

// ---- MATHB2: the generic call's arguments ----

/// The generic call of the pair takes the nodes the intrinsic arm evaluated
/// its arguments to, by position: in `Math.max(h(f()), f())` the second
/// argument is the outer `f()` (f runs twice, h once), not the `f()` inside
/// `h(…)` that is structurally equal to it.
#[test]
fn mathb2_pair_args_by_position() {
    let src = "let n = Number(process.argv[2]);\nconst f = () => ++n;\nconst h = (y: number) => y * 2;\nconsole.log(Math.max(h(f()), f()));\n";
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").expect("parses"),
    ));
    let mut set = perry_hir::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(ast, &mut set);
    set.namespace_pairs = true;
    perry_hir::set_patched_builtins(std::sync::Arc::new(set));
    let hir = perry_hir::lower_module(ast, "t", "t.ts");
    perry_hir::clear_patched_builtins();
    let hir = Box::leak(Box::new(hir.expect("lowers")));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.ns_pairs = true;
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    let pair: Vec<crate::model::CallId> =
        g.m.ns_pair_calls
            .iter()
            .filter(|(_, &(_, k))| g.m.atoms[k as usize] == "max")
            .map(|(&c, _)| c)
            .collect();
    assert!(!pair.is_empty());
    let fr: Vec<crate::model::NodeId> =
        g.m.calls
            .iter()
            .filter(|c| c.args.is_empty())
            .map(|c| c.ret)
            .collect();
    let hc: Vec<&crate::model::CallSite> =
        g.m.calls
            .iter()
            .filter(|c| c.args.len() == 1 && fr.contains(&c.args[0]))
            .collect();
    assert_eq!(fr.len(), 2, "f runs twice");
    assert_eq!(hc.len(), 1, "h runs once");
    for c in pair {
        let gc = &g.m.calls[c as usize];
        eprintln!(
            "MB2 generic {c} args {:?} f rets {fr:?} h args {:?}",
            gc.args, hc[0].args
        );
        assert_eq!(gc.args.len(), 2);
        assert_eq!(gc.args[0], hc[0].ret, "first argument: h's result");
        assert!(
            fr.contains(&gc.args[1]) && gc.args[1] != hc[0].args[0],
            "second argument: the outer f()"
        );
    }
}

/// MATHB2 4: knob off, the pairs main emits (a `globalThis[k] = v` store
/// guards every direct namespace call by the shadow bit) keep main's walk of
/// both arms: no pair call is registered, and the dump (printed `MB2DUMP`)
/// equals main's for the same source (compared against the base tree).
#[test]
fn mathb2_main_pair_walk_unchanged() {
    let src = "const k = String(process.argv[2]);\n(globalThis as any)[k] = 3;\nclass P { v: number; constructor(x: number) { this.v = Math.sin(x); } }\nconst p = new P(Number(process.argv[2]));\nconsole.log(Math.max(p.v, Math.abs(p.v)), Object.keys(p).join(\",\"));\n";
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut set = perry_hir::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(&module, &mut set);
    perry_hir::set_patched_builtins(std::sync::Arc::new(set));
    let guarded = perry_hir::patched_builtins::namespace_call_guarded();
    let hir = perry_hir::lower_module(&module, "t", "t.ts");
    perry_hir::clear_patched_builtins();
    assert!(guarded, "the store guards the namespace calls");
    let hir = hir.expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    let pf = compute(&[("t.ts".to_string(), &hir)], &cfg);
    for l in pf.dump_text().lines() {
        eprintln!("MB2DUMP {l}");
    }
}

// ---- MATHB2 5: the verdict, refused and proven ----

#[test]
fn mathb2_keep_computed_unknown_key() {
    absent(
        &p("const k = String(process.argv[2]);\n(Math as any)[k] = f;"),
        "sin",
        "Math.sin",
    );
}

#[test]
fn mathb2_keep_global_math_replaced() {
    absent(
        &p("(globalThis as any).Math = { sin: f, cos: f };"),
        "sin",
        "Math.sin",
    );
}

/// Flow-insensitive: a write placed after the call in program order is
/// still a write of the member the call may read (a later run, a loop).
#[test]
fn mathb2_keep_write_after_the_call() {
    let src = format!("{X}console.log(Math.sin(x), f(1));\n(Math as any).sin = f;\n");
    absent(&src, "sin", "Math.sin");
}

/// A local object with a member named like the namespace's is no write of
/// the namespace.
#[test]
fn mathb2_unrelated_object_written() {
    let src = p("const Math2: any = { sin: 0 };\nMath2.sin = f;\nconsole.log(Math2.sin(String(Math.random())));");
    present(&src, "sin", "Math.sin");
}

/// STOP10 Q1 (CWN ruling 2026-11-24; owner 11-24 decision 1 drops the E20
/// hold): a program store onto a builtin constructor under an unknown key
/// through a handed reference (`put(Array, k, f)`) may replace any of its
/// statics. The direct call `Array.from(…)` evaluates the member and calls
/// that value (ECMA-262 13.3.6.1), maybe `f`: its member read is no
/// NativeMember, the driver takes no intrinsic arm, and the call's result
/// may be `f`'s. RED before: `ns_native` counted the native writes only,
/// not the program's own stores into the constructor's fields, so the
/// site was NativeMember and the driver called the intrinsic.
#[test]
fn stop10_store_through_handed_constructor_reaches_its_statics() {
    let mut bad = Vec::new();
    for call in [
        "Array.from([1] as any)",
        "Array.isArray([1])",
        "Array.of(1)",
        "Object.keys({ a: 1 })",
        "Number.isInteger(1)",
        "String.fromCharCode(65)",
        "Promise.resolve(1)",
    ] {
        let (ns, rest) = call.split_once('.').unwrap();
        let member = rest.split_once('(').unwrap().0;
        let src = format!(
            "function put(o: any, k: string, v: any): void {{ o[k] = v; }}\n\
             put({ns}, process.argv[2] ?? \"q\", function () {{ return {{ qq: 7 }}; }});\n\
             const r: any = /*A*/ {call};\nconsole.log(r.qq);\n"
        );
        let pf = solve(&src);
        let at = src.find("/*A*/").unwrap() as u32;
        let Some(site) = pf
            .sites
            .iter()
            .filter(|s| s.module == 0 && s.is_call && s.key == member && s.byte_offset >= at)
            .min_by_key(|s| s.byte_offset)
        else {
            bad.push(format!("{call}: no call site"));
            continue;
        };
        if matches!(site.verdict, Verdict::NativeMember) {
            bad.push(format!("{call}: the member read is NativeMember"));
        }
        if crate::native_member_calls_of(&pf, 0, true).contains(&site.byte_offset) {
            bad.push(format!("{call}: the driver takes the intrinsic arm"));
        }
        let qq: Vec<String> = pf
            .sites
            .iter()
            .filter(|s| s.module == 0 && s.key == "qq" && !s.access.is_store())
            .map(|s| format!("{:?} {:?}", s.verdict, s.receivers))
            .collect();
        // The stored function returns an object literal (`__AnonShape_*`):
        // the read of `qq` on the result must see it. Until the E20 realm
        // hold is dropped (owner 11-24 decision 1, the next commit) the
        // Promise static's load is the realm-held field and the read is TOP
        // (unproven; it includes the stored value).
        let seen = |x: &String| x.contains("__AnonShape_") || x.contains("(TOP)");
        if !qq.iter().any(seen) {
            bad.push(format!("{call}: the stored function's result is not read: {qq:?}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// KEEP: with no store onto the constructor its static reads the intrinsic
/// and the driver takes the intrinsic arm.
#[test]
fn stop10_unwritten_constructor_static_stays_native() {
    let src = "const r: any = /*A*/ Array.from([1] as any);\nconsole.log(r.length);\n";
    let pf = solve(src);
    let at = src.find("/*A*/").unwrap() as u32;
    let site = pf
        .sites
        .iter()
        .filter(|s| s.module == 0 && s.is_call && s.key == "from" && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .expect("Array.from site");
    assert!(
        crate::native_member_calls_of(&pf, 0, true).contains(&site.byte_offset),
        "Array.from: {:?}",
        site.verdict
    );
}

/// CWN ruling 2026-11-24 (STOP10): a refusal always wins. A NativeMember
/// call site whose namespace twin refuses the callee is no native call,
/// with namespace pairs on or off (the doc of `native_member_calls_of`).
/// RED before: the refusal was counted only for twins that were not
/// NativeMember, and not at all with pairs off.
#[test]
fn stop10_refused_twin_removes_a_native_member_call() {
    let src = "const r: any = /*A*/ Array.from([1] as any);\nconsole.log(r.length);\n";
    let mut pf = solve(src);
    let at = src.find("/*A*/").unwrap() as u32;
    let off = pf
        .sites
        .iter()
        .filter(|s| s.module == 0 && s.is_call && s.key == "from" && s.byte_offset >= at)
        .map(|s| s.byte_offset)
        .min()
        .expect("Array.from site");
    let mut seeded = false;
    for s in pf.sites.iter_mut().filter(|s| s.module == 0 && s.is_call && s.byte_offset == off) {
        seeded |= matches!(s.verdict, Verdict::NativeMember);
        s.verdict = Verdict::NativeMember;
        s.intrinsic_callee = Some(crate::derive::IntrinsicCallee::No("test refusal".into()));
    }
    assert!(seeded, "the unwritten Array.from site is NativeMember");
    for pairs in [true, false] {
        assert!(
            !crate::native_member_calls_of(&pf, 0, pairs).contains(&off),
            "pairs={pairs}: a refused twin must remove the native call"
        );
    }
}

