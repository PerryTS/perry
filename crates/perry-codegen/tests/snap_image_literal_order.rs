use perry_codegen::{compile_module, CompileOptions};
use perry_hir::{Class, Module};

fn class(id: u32, name: &str) -> Class {
    Class {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
        decl_stmt: None,
    }
}

/// The name of the function that holds the first line containing `needle`.
fn holder(ir: &str, needle: &str) -> String {
    let mut current = String::new();
    for line in ir.lines() {
        if line.starts_with("define ") {
            current = line.to_string();
        }
        if line.contains(needle) {
            return current;
        }
    }
    panic!("{needle} not found");
}

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
    let mut module = Module::new("entry.ts");
    module.classes.push(class(7, "__AnonShape_0000000000000007"));
    module.classes.push(class(8, "Declared"));
    String::from_utf8(compile_module(&module, opts).unwrap()).unwrap()
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
    // A literal-shape class is registered by the literal pool; a declared
    // class stays with the late class chunks.
    for call in ["js_register_class_id", "js_register_class_length"] {
        let anon = holder(&on, &format!("call void @{call}(i32 7"));
        assert!(anon.contains("_literal_chunk"), "{call}: {anon}");
        let declared = holder(&on, &format!("call void @{call}(i32 8"));
        assert!(!declared.contains("_literal_chunk"), "{call}: {declared}");
    }
    // Knob off: no image calls at all, and the registrations stay late.
    let off = ir(false);
    for call in ["js_register_class_id", "js_register_class_length"] {
        let anon = holder(&off, &format!("call void @{call}(i32 7"));
        assert!(!anon.contains("_literal_chunk"), "{call}: {anon}");
    }
    assert!(!off.contains("js_snap_image_begin") && !off.contains("js_snap_image_cut"));
}
