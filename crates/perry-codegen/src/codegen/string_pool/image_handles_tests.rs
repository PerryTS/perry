use super::emit_string_pool;
use crate::module::LlModule;
use crate::strings::StringPool;
use std::collections::{HashMap, HashSet};

fn emitted(target: &str, agent_strings_tls: bool) -> String {
    let mut strings = StringPool::with_prefix("probe".into());
    strings.intern("literal");
    let mut module = LlModule::new(target);
    emit_string_pool(
        &mut module,
        &strings,
        "probe",
        agent_strings_tls,
        "executable",
        &[],
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::<String, &perry_hir::Class>::new(),
        &[],
        &crate::layout_families::ProgramLayouts::default(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashSet::new(),
        &HashMap::new(),
        &HashSet::new(),
        &[],
        &HashSet::new(),
        &HashSet::new(),
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &[],
        &HashSet::new(),
        &HashSet::new(),
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &[],
        &HashSet::new(),
    );
    module.to_ir()
}

#[test]
fn executable_image_handles_are_boxed_at_runtime_for_relocation_limited_targets() {
    for target in [
        "arm64-apple-macosx15.0.0",
        "x86_64-apple-darwin",
        "aarch64-apple-ios-simulator",
        "arm64-apple-tvos",
        "arm64-apple-watchos",
        "arm64-apple-xros",
        "wasm32-wasip2",
        "wasm32-unknown-unknown",
    ] {
        let ir = emitted(target, false);
        assert!(
            ir.contains("@perry_image_literal_0_6c69746572616c = weak_odr constant"),
            "{target}\n{ir}"
        );
        assert!(
            ir.contains("@probe_.str.0.handle = internal global double 0.0"),
            "{target}\n{ir}"
        );
        assert!(
            ir.contains("getelementptr i8, ptr @perry_image_literal_0_6c69746572616c, i64 8"),
            "{target}"
        );
        assert!(ir.contains("call double @js_nanbox_string(i64"), "{target}");
        assert!(ir.contains("store double %"), "{target}");
        assert!(
            ir.contains("@__perry_agent_strings_ready_probe = internal global i8 0"),
            "{target}"
        );
        assert!(
            ir.contains("call void @__perry_agent_strings_probe_literal_chunk0()"),
            "{target}"
        );
        assert!(
            !ir.contains("add (i64 ptrtoint"),
            "{target} emitted a tagged pointer relocation"
        );
        assert!(!ir.contains("js_string_pool_atom"), "{target}");
    }

    let linux = emitted("aarch64-unknown-linux-gnu", false);
    assert!(linux.contains(
        "@probe_.str.0.handle = internal externally_initialized constant double bitcast"
    ));
    assert!(linux.contains("add (i64 ptrtoint"));
    assert!(linux.contains("@perry_image_literal_0_6c69746572616c = weak_odr constant"));
    assert!(!linux.contains("js_nanbox_string"));
    assert!(!linux.contains("__perry_agent_strings_ready_probe"));
}

#[test]
fn runtime_boxed_image_handles_and_ready_guard_follow_agent_tls_mode() {
    let single_agent = emitted("arm64-apple-macosx15.0.0", false);
    assert!(single_agent.contains("@probe_.str.0.handle = internal global double 0.0"));
    assert!(single_agent.contains("@__perry_agent_strings_ready_probe = internal global i8 0"));

    let worker_agent = emitted("arm64-apple-macosx15.0.0", true);
    assert!(worker_agent.contains("@probe_.str.0.handle = internal thread_local global double 0.0"));
    assert!(worker_agent
        .contains("@__perry_agent_strings_ready_probe = internal thread_local global i8 0"));
}
