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
    assert!(!v.is_empty() && v.iter().all(|x| *x == want), "{key}: {v:?}\n{src}");
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
    present(&p("const c = Math.sin;\nconsole.log(c(x));"), "()", "Math.sin");
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
    absent(&p("(globalThis as any)['Ma' + 'th'].sin = f;"), "sin", "Math.sin");
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
    absent(&p("Object.defineProperty(Math, 'sin', { value: f });"), "sin", "Math.sin");
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
    absent(&p("if (process.argv[2]) (Math as any).sin = f;"), "sin", "Math.sin");
}

#[test]
fn mathb1_keep_unknown_receiver_unknown_key() {
    let src = p("const o: any = (globalThis as any)[process.argv[2]];\no[process.argv[3]] = f;");
    absent(&src, "sin", "Math.sin");
}

#[test]
fn mathb1_keep_proxy_store() {
    absent(&p("const q: any = new Proxy(Math, {});\nq.sin = f;"), "sin", "Math.sin");
}

#[test]
fn mathb1_keep_alias_after_write() {
    let src = p("const m: any = Math;\nm.sin = f;\nconst c = Math.sin;\nconsole.log(c(x));");
    absent(&src, "()", "Math.sin");
}

// ---- fixtures of the stopped line (s/me1/fx, s/me6/v) ----

#[test]
fn mathb1_fx_d1() {
    absent(r#"function w(o: any, k: string) { o[k] = (x: number) => 42; }
w(Math, process.argv[2]); const f = Math.sin; console.log(f(1));
"#, "()", "Math.sin");
}

#[test]
fn mathb1_fx_d2() {
    absent(r#"const g: any = globalThis; g["Ma" + "th"].sin = (x: number) => 42; const f = Math.sin; console.log(f(1));
"#, "()", "Math.sin");
}

#[test]
fn mathb1_fx_d3() {
    absent(r#"const M: any = Math; M.sin = (x: number) => 42; const f = Math.sin; console.log(f(1));
"#, "()", "Math.sin");
}

#[test]
fn mathb1_fx_d4() {
    absent(r#"function w(o: any, k: string) { o[k] = (x: number) => 42; }
const f = Math.sin; w(Math, process.argv[2]); console.log(f(1), Math.sin(1));
"#, "sin", "Math.sin");
}

#[test]
fn mathb1_fx_d4_alias() {
    absent(r#"function w(o: any, k: string) { o[k] = (x: number) => 42; }
const f = Math.sin; w(Math, process.argv[2]); console.log(f(1), Math.sin(1));
"#, "()", "Math.sin");
}

#[test]
fn mathb1_fx_d5() {
    absent(r#"function w(o:any,k:string){o[k]=(x:number)=>42}
w(Math,process.argv[2]); console.log(Math.sin(1))
"#, "sin", "Math.sin");
}

#[test]
fn mathb1_fx_g6() {
    present(r#"function keep( o: any ) { return o }
keep( globalThis );
class Q { a: number; b: number; constructor() { this.a = 1; this.b = 2; Object.defineProperty( this, "id", { value: 7 } ) } }
const qs: Q[] = []; for ( let i = 0; i < 100; i++ ) qs.push( new Q() ); const o = { z: 3 };
let t = 0; for ( let r = 0; r < 20000; r++ ) for ( let i = 0; i < 100; i++ ) { const q = qs[ i ]; t += q.a * q.b + o.z }
console.log( t, (qs[ 0 ] as any).id );
"#, "defineProperty", "Object.defineProperty");
}

#[test]
fn mathb1_fx_g6n() {
    present(r#"function keep( o: any ) { return o }
class Q { a: number; b: number; constructor() { this.a = 1; this.b = 2; Object.defineProperty( this, "id", { value: 7 } ) } }
const qs: Q[] = []; for ( let i = 0; i < 100; i++ ) qs.push( new Q() ); const o = { z: 3 };
let t = 0; for ( let r = 0; r < 20000; r++ ) for ( let i = 0; i < 100; i++ ) { const q = qs[ i ]; t += q.a * q.b + o.z }
console.log( t, (qs[ 0 ] as any).id );
"#, "defineProperty", "Object.defineProperty");
}

#[test]
fn mathb1_fx_g7() {
    absent(r#"function keep( o: any ) { return o }
keep( globalThis );
const box: any = { r: null }; function pick(): any { return box.r }
class Q { a: number; b: number; constructor() { this.a = 1; this.b = 2; Object.defineProperty( this, "id", { value: 7 } ) } }
const qs: Q[] = []; for ( let i = 0; i < 100; i++ ) qs.push( new Q() );
box.r = Object; const k = process.argv.length > 99 ? "x" : "defineProperty";
const orig = Object.defineProperty; const o = pick(); o[ k ] = function ( t: any, kk: any, d: any ) { t.seen = kk; return t };
const q2: any = new Q(); o[ k ] = orig;
let t = 0; for ( let r = 0; r < 2000; r++ ) for ( let i = 0; i < 100; i++ ) { const q = qs[ i ]; t += q.a * q.b }
console.log( t, (qs[ 0 ] as any).id, q2.id, q2.seen, Object.keys( q2 ).join( "," ), "id" in q2 );
"#, "defineProperty", "Object.defineProperty");
}

#[test]
fn mathb1_fx_g7c() {
    absent(r#"function keep( o: any ) { return o }
keep( globalThis );
const box: any = { r: null }; function pick(): any { return box.r }
class Q { a: number; b: number; constructor() { this.a = 1; this.b = 2; Object.defineProperty( this, "id", { value: 7 } ) } }
const qs: Q[] = []; for ( let i = 0; i < 100; i++ ) qs.push( new Q() );
box.r = Object; const k = process.argv.length > 99 ? "x" : "defineProperty";
const orig = Object.defineProperty; const o = box.r; o[ k ] = function ( t: any, kk: any, d: any ) { t.seen = kk; return t };
const q2: any = new Q(); o[ k ] = orig;
let t = 0; for ( let r = 0; r < 2000; r++ ) for ( let i = 0; i < 100; i++ ) { const q = qs[ i ]; t += q.a * q.b }
console.log( t, (qs[ 0 ] as any).id, q2.id, q2.seen, Object.keys( q2 ).join( "," ), "id" in q2 );
"#, "defineProperty", "Object.defineProperty");
}

#[test]
fn mathb1_fx_g8() {
    absent(r#"function keep( o: any ) { return o }
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
"#, "defineProperty", "Object.defineProperty");
}

#[test]
fn mathb1_fx_g10() {
    absent(r#"function keep( o: any ) { return o }
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
"#, "defineProperty", "Object.defineProperty");
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
    let bad: Vec<&str> = d.lines().filter(|l| l.starts_with("S ") && l.contains("verdict=no")).collect();
    eprintln!("MB1B {bad:?}");
    assert!(bad.is_empty(), "{bad:?}");
    assert!(ic(src, "defineProperty").iter().all(|v| v == "exact Object.defineProperty"));
}

