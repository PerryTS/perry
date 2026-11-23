//! A call of `Object.defineProperty` that lowering leaves as a dynamic call
//! (a computed-key define on the global object anywhere in the program, here
//! in a never-called helper, makes the patched-builtins scan treat `Object`
//! as possibly replaced) runs the direct call's steps: under a String
//! constant key it defines that key by name, as `Expr::ObjectDefineProperty`
//! does, so the global object takes no any-key write and later reads off it
//! (`console`) keep their known values. A key that is not a constant keeps
//! the computed-key path.
use super::*;

/// Lower `src` the way the compiler does (the patched-builtins scan first),
/// so the define below is a value call.
fn facts(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let mut pb = perry_hir::patched_builtins::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(&module, &mut pb);
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    perry_hir::patched_builtins::set_patched_builtins(std::sync::Arc::new(pb));
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    perry_hir::patched_builtins::clear_patched_builtins();
    crate::compute(&[(path, &hir)], &cfg)
}

fn program(key: &str, call_helper: bool) -> String {
    let call = match call_helper {
        true => "define(String(a.x), () => 1);\n",
        false => "",
    };
    format!(
        "const g = globalThis as Record<string, unknown>;\n\
         class Vec {{\n\
           x: number = 0;\n\
           add(v: Vec): Vec {{ this.x += v.x; return this; }}\n\
         }}\n\
         function define(name: string, get: () => unknown): void {{\n\
           Object.defineProperty(g, name, {{ get, configurable: true }});\n\
         }}\n\
         Object.defineProperty(g, {key}, {{ configurable: true, writable: true, value: {{\n\
           userAgent: \"perry\",\n\
           clipboard: {{ readText: (): Promise<string> => Promise.resolve(\"\") }},\n\
         }} }});\n\
         const a = new Vec();\n\
         a.add(new Vec());\n\
         {call}\
         console.log(a.x);\n"
    )
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !s.is_empty() && s.iter().all(|s| !matches!(s.verdict, Verdict::No(_)))
}

/// The ladder's r36 shape: the never-called helper writes nothing, and the
/// value-call define under `"navigator"` writes only that key, so the
/// program sites stay proven.
#[test]
fn value_call_define_named_key_keeps_sites() {
    let pf = facts(&program("\"navigator\"", false));
    for key in ["x", "add", "log"] {
        assert!(proven(&pf, key), "{key}\n{}", pf.dump_text());
    }
}

/// KEEP: the helper called with a key that is not a constant defines under
/// a computed key (E5 on the global object), as before.
#[test]
fn value_call_define_computed_key_stays_any_key() {
    let pf = facts(&program("\"navigator\"", true));
    assert!(!proven(&pf, "log"), "{}", pf.dump_text());
}
