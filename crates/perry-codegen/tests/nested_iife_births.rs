use perry_codegen::{module_birth_shapes, CompileOptions};
use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;
use std::time::{Duration, Instant};

/// Births pre-pass over deeply nested IIFEs (JSX/Solid component trees lower
/// to these). Each IIFE body is walked once with its parameters bound; walking
/// it again through the callee doubled the work per level, so 40 levels never
/// finished (opencode packages/tui/src/app.tsx ran away to 140+ GB).
#[test]
fn nested_iifes_walk_each_body_once() {
    // Debug HIR lowering needs the same stack used by its other fixtures;
    // the CLI gives compilation workers 128 MiB. Keep the birth-walk time
    // bound below unchanged rather than timing a libtest stack overflow.
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(check_nested_iife_births)
        .expect("spawn lowering fixture")
        .join()
        .expect("nested IIFE lowering fixture");
}

fn check_nested_iife_births() {
    let mut body = "{ const o = { x: 1, y: 2 }; return o.x + a39; }".to_string();
    for i in (0..39).rev() {
        body = format!(
            "{{ return ((a{n}: number) => {body})({n}) + a{i}; }}",
            n = i + 1
        );
    }
    let source = format!("export const r = ((a0: number) => {body})(0);\nconsole.log(r);\n");
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(&source, "nested_iife.ts", &mut cache).unwrap();
    let module = lower_module(&parsed.module, "nested_iife", "nested_iife.ts").unwrap();
    let options = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".into(),
        disable_constfn_shapes: false,
        ..Default::default()
    };
    let started = Instant::now();
    let births = module_birth_shapes(&module, options).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "births pre-pass took {:?} over 40 nested IIFEs",
        started.elapsed()
    );
    assert!(
        births.iter().any(|b| b.shape.key_count == 2),
        "the innermost literal {{x, y}} is still a birth"
    );
}
