use perry_codegen::{compile_module, CompileOptions};
use perry_hir::Module;

fn ir(image: bool) -> String {
    if image {
        std::env::set_var("PERRY_SNAP_IMAGE_RUN", "1");
    } else {
        std::env::remove_var("PERRY_SNAP_IMAGE_RUN");
    }
    let opts = CompileOptions {
        emit_ir_only: true,
        is_entry_module: true,
        non_entry_module_prefixes: vec!["eager_a_ts".into(), "eager_b_ts".into(), "lazy_ts".into()],
        deferred_module_prefixes: std::sync::Arc::new(["lazy_ts".to_string()].into_iter().collect()),
        ..Default::default()
    };
    String::from_utf8(compile_module(&Module::new("entry.ts"), opts).unwrap()).unwrap()
}

/// One test: the knob is process-wide, so both arms run in sequence.
#[test]
fn image_begin_follows_every_eager_literal_pool_and_precedes_every_body() {
    let on = ir(true);
    let begin = on.find("call void @js_snap_image_begin(").expect("begin call");
    for p in ["eager_a_ts", "eager_b_ts"] {
        let pool = on
            .find(&format!("ptr @__perry_prepare_literals_{p} to i64"))
            .expect("eager pool prepared");
        let body = on.find(&format!("call void @{p}__init()")).expect("eager body");
        assert!(pool < begin, "{p}: literal pool must be prepared before the baseline");
        assert!(begin < body, "{p}: the baseline must precede the module body");
    }
    assert!(!on.contains("ptr @__perry_prepare_literals_lazy_ts to i64"));
    assert!(begin < on.find("call void @js_snap_image_cut(").expect("cut call"));
    // Knob off: no image calls at all.
    let off = ir(false);
    assert!(!off.contains("js_snap_image_begin") && !off.contains("js_snap_image_cut"));
}
