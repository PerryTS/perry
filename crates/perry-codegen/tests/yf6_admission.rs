use perry_codegen::{compile_module, CompileOptions};
use perry_hir::types::Type;

const YF6_REAL: &str = include_str!("fixtures/yf6_real.js");

fn defined_function_ir_section<'a>(ir: &'a str, symbol: &str) -> &'a str {
    let needle = format!("@{symbol}(");
    let mut search_start = 0;
    let start = loop {
        let relative = ir[search_start..]
            .find(&needle)
            .unwrap_or_else(|| panic!("function `{symbol}` definition not found in IR:\n{ir}"));
        let symbol_start = search_start + relative;
        let line_start = ir[..symbol_start]
            .rfind('\n')
            .map_or(0, |newline| newline + 1);
        if ir[line_start..symbol_start]
            .trim_start()
            .starts_with("define ")
        {
            break line_start;
        }
        search_start = symbol_start + needle.len();
    };
    let rest = &ir[start..];
    let end = rest.find("\n}\n").map_or(rest.len(), |close| close + 3);
    &rest[..end]
}

#[test]
fn real_yf6_erased_predicate_gets_typed_i1_clone() {
    // Keep YF6 byte-for-byte identical to the cc bundle. The typed caller is
    // appended separately so codePointAt's possible `undefined` exercises the
    // public guard instead of proving a raw-f64 direct call.
    let source =
        format!("{YF6_REAL}\nfunction caller(s: string){{return YF6(s.codePointAt(0))}}\n");

    let ir = std::thread::Builder::new()
        .name("real-yf6-codegen".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let ast = perry_parser::parse_typescript(&source, "yf6_admission.ts")
                .expect("real YF6 should parse");
            let hir = perry_hir::lower_module(&ast, "yf6_admission.ts", "yf6_admission.ts")
                .expect("real YF6 should lower");

            let yf6 = hir
                .functions
                .iter()
                .find(|function| function.name == "YF6")
                .expect("YF6 should be a top-level HIR function");
            assert_eq!(
                yf6.return_type,
                Type::Boolean,
                "the full logical chain must retain its inferred Boolean return type"
            );

            let options = CompileOptions {
                emit_ir_only: true,
                ..CompileOptions::default()
            };
            String::from_utf8(compile_module(&hir, options).expect("real YF6 should codegen"))
                .expect("LLVM IR should be UTF-8")
        })
        .expect("spawn YF6 codegen thread")
        .join()
        .expect("YF6 codegen thread panicked");

    let public = "perry_fn_yf6_admission_ts__YF6";
    let typed = "perry_fn_yf6_admission_ts__YF6$typed_i1";
    let generic = "perry_fn_yf6_admission_ts__YF6$generic";
    let caller = "perry_fn_yf6_admission_ts__caller";
    let typed_ir = defined_function_ir_section(&ir, typed);
    let generic_ir = defined_function_ir_section(&ir, generic);
    let wrapper_ir = defined_function_ir_section(&ir, public);
    let caller_ir = defined_function_ir_section(&ir, caller);

    assert!(
        typed_ir.starts_with(&format!("define internal i1 @{typed}(double ")),
        "YF6 should have an f64-to-i1 typed clone:\n{typed_ir}"
    );
    assert_eq!(
        typed_ir.matches("fcmp oge double").count(),
        76,
        "YF6 clone should lower every >= natively:\n{typed_ir}"
    );
    assert_eq!(
        typed_ir.matches("fcmp ole double").count(),
        76,
        "YF6 clone should lower every <= natively:\n{typed_ir}"
    );
    assert_eq!(
        typed_ir.matches("fcmp oeq double").count(),
        60,
        "YF6 clone should lower every === natively:\n{typed_ir}"
    );
    assert!(
        !typed_ir.contains("@js_rel_"),
        "YF6 clone must not retain generic relational calls:\n{typed_ir}"
    );

    assert!(
        generic_ir.contains("call double @js_rel_ge(")
            && generic_ir.contains("call double @js_rel_le("),
        "YF6's generic JSValue fallback must remain intact:\n{generic_ir}"
    );
    assert!(
        wrapper_ir.contains(", 32761")
            && wrapper_ir.contains(&format!("call i1 @{typed}(double "))
            && wrapper_ir.contains(&format!("call double @{generic}(double ")),
        "YF6's public wrapper must guard the value and retain both paths:\n{wrapper_ir}"
    );
    assert!(
        caller_ir.contains("call double @js_string_code_point_at(")
            && caller_ir.contains(&format!("call double @{public}(double "))
            && !caller_ir.contains(&format!("call i1 @{typed}(double ")),
        "codePointAt's undefined case must enter through YF6's guarded wrapper:\n{caller_ir}"
    );
}
