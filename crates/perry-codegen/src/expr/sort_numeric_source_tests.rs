//! Source-to-operation witnesses: a semantic proof must survive the frontend,
//! solved callee facts and the folded-method guard.
use crate::{compile_module, CompileOptions, ElisionFacts, FactsBody, ProgramFactsView};
use perry_hir::{Expr, Stmt};
use std::sync::Arc;
fn ir(source: &str, close: bool) -> String {
    let source = source.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&source, "sign.ts", &mut cache).unwrap();
            let m = perry_hir::lower_module(&parsed.module, "sign", "sign.ts").unwrap();
            let mut facts = ElisionFacts::default();
            if close {
                for s in &m.init {
                    if let Stmt::Let {
                        id,
                        init:
                            Some(Expr::Closure {
                                func_id, params, ..
                            }),
                        ..
                    } = s
                    {
                        facts.local_bodies.insert(
                            *id,
                            vec![FactsBody {
                                func_id: *func_id,
                                params: params.len() as u32,
                                closure: true,
                            }],
                        );
                    }
                }
            }
            if close && facts.local_bodies.len() == 1 {
                let bodies = facts.local_bodies.values().next().unwrap().clone();
                for f in &m.functions {
                    for p in &f.params {
                        if p.name == "f" {
                            facts.local_bodies.insert(p.id, bodies.clone());
                        }
                    }
                }
            }
            let opts = CompileOptions {
                emit_ir_only: true,
                program_facts: ProgramFactsView {
                    elision: Arc::new(facts),
                    ..Default::default()
                },
                ..Default::default()
            };
            String::from_utf8(compile_module(&m, opts).unwrap()).unwrap()
        })
        .unwrap()
        .join()
        .unwrap()
}
fn selected(ir: &str) -> bool {
    ir.contains("call i64 @js_array_sort_numeric_order(")
        || ir.contains("call i64 @js_array_sort_key_order(")
}
#[test]
fn numeric_sign_source_forms_select_the_certified_operation() {
    for source in [
        "const cmp=(a,b)=>a-b; function run(xs:any[],f:any) { xs.sort((a,b)=>f(a,b)); } run([3,1,2],cmp);",
        "function cmp(a,b,unused) { return a-b; } const xs=[3,1,2]; xs.sort(cmp);",
        "const xs=[3,1,2]; xs.sort((a,b) => { const d=a-b; if(d<0) return -1; if(d>0) return 1; return 0; });",
        "const xs=[3,1,2]; xs.sort((a,b) => a-b || 0);",
        "const xs=[3,1,2]; xs.sort((a,b) => (a>b)-(a<b));",
        "const xs=[3,1,2]; xs.sort((a,b) => a<b ? -37 : a>b ? 42 : 0);",
        "function inner(a,b) { return a===b ? 0 : a===undefined ? -1 : b===undefined ? 1 : a<b ? -1 : 1; } function outer(a,b) { return inner(a,b); } const xs=[3,1,2]; xs.sort(outer);",
        "const xs = [3,1,2]; xs.sort((a,b) => { return a-b; });",
        "const xs = [3,1,2]; xs.sort((a,b) => { if(a<b) return -1; if(a>b) return 1; return 0; });",
        "const xs = [3,1,2]; xs.sort((a,b) => a<b ? -1 : a>b ? 1 : 0);",
        "const xs = [3,1,2]; xs.sort((a,b) => a===b ? 0 : a<b ? -1 : 1);",
        "const xs = [3,1,2]; xs.sort((a,b) => Number(a)-Number(b));",
        "const xs = [3,1,2]; xs.sort((a,b) => +a - +b);",
        "function cmp(a,b) { if(a<b) return -1; if(a>b) return 1; return 0; } const xs = [3,1,2]; xs.sort(cmp);",
        "function cmp(a,b) { return a-b; } const xs = [3,1,2]; xs.sort((a,b) => cmp(a,b));",
        "const cmp = (a,b) => a-b; const xs = [3,1,2]; xs.sort((a,b) => cmp(a,b));",
        "const project = x => x.k; const xs = [{k:3},{k:1}]; xs.sort((a,b) => project(a)-project(b));",
    ] {
        let output = ir(source,true);
        assert!(selected(&output),"no proof for {source}\n{output}");
    }
}
#[test]
fn numeric_sign_source_effects_inconsistency_unknown_and_tdz_keep_callbacks() {
    for (source, close) in [
        ("const xs=[-2,-1,1,2]; xs.sort((a,b)=>{ const scale=1/(a<0?-0:0); return a<b?-scale:a>b?scale:0; });",true),
        ("const cmp=(a,b)=>a-b; function run(xs:any[]=[2,1].sort((a,b)=>f(a,b)),f:any=cmp) {} run();",true),
        ("const xs=[{k:1,j:2},{k:1,j:1}]; xs.sort((a,b)=>a.k!==b.k?a.k-b.k:a.j-b.j);",true),
        ("const xs = [3,1,2]; xs.sort((a,b) => a<b ? -1 : 1);",true),
        ("let count=0; const xs = [3,1,2]; xs.sort((a,b) => { count++; return a-b; });",true),
        ("const offset=1; const xs = [3,1,2]; xs.sort((a,b) => a-b+offset);",true),
        ("const xs = [{k:3,j:1},{k:1,j:3}]; xs.sort((a,b) => a.k-b.j);",true),
        ("function cmp(a,b) { console.log(a); return a-b; } const xs = [3,1,2]; xs.sort((a,b) => cmp(a,b));",true),
        ("const cmp = (a,b) => a-b; const xs = [3,1,2]; xs.sort((a,b) => cmp(a,b));",false),
        ("const xs = [3,1,2]; xs.sort((a,b) => cmp(a,b)); const cmp = (a,b) => a-b;",true),
        ("const xs = [3,1,2]; xs.sort((a,b) => { const d=a-b; return d===0 ? 0 : d<0 ? -1 : 1; });",true),
    ] {
        let output = ir(source,close);
        assert!(!selected(&output),"unsound proof for {source}\n{output}");
    }
}
