//! Module-owned records and static dependency visitors for async evaluation.
use super::*;

pub(super) fn emit_record(
    llmod: &mut LlModule,
    hir: &HirModule,
    prefix: &str,
    cross: &CrossModuleCtx,
) {
    // repr(C) AsyncModule is sixteen eight-byte words. The accessor computes
    // addresses at execution time so worker TLS never appears in a relocation.
    let record = format!("__perry_async_module_{prefix}");
    llmod.add_internal_module_state_global(&record, "[16 x i64]", "zeroinitializer");
    let body = format!("{prefix}__init_body");
    let visit = format!("{prefix}__async_dependencies");
    llmod.declare_function(&body, DOUBLE, &[]);
    for dep in &cross.module_init_deps {
        llmod.declare_function(&format!("{dep}__async_record"), I64, &[]);
    }
    let namespace = if !cross.namespace_entries.is_empty() || cross.is_dynamic_import_target {
        format!("ptrtoint (ptr @__perry_ns_{prefix} to i64)")
    } else {
        "0".to_string()
    };
    let flags = u64::from(hir.has_top_level_await) | (u64::from(cross.module_graph_has_tla) << 1);
    let accessor = llmod.define_function(&format!("{prefix}__async_record"), I64, vec![]);
    let blk = accessor.create_block("entry");
    // Cyclic dependencies can call hoisted functions before this module's
    // body. Literal preparation contains no user statements or class statics.
    blk.call_void(&format!("__perry_prepare_literals_{prefix}"), &[]);
    let addr = format!("ptrtoint (ptr @{record} to i64)");
    blk.call_void(
        "js_async_module_configure",
        &[
            (I64, &addr),
            (I64, &format!("ptrtoint (ptr @{body} to i64)")),
            (I64, &format!("ptrtoint (ptr @{visit} to i64)")),
            (I64, &namespace),
            (I64, &flags.to_string()),
        ],
    );
    blk.ret(I64, &addr);
    let visitor = llmod.define_function(
        &visit,
        VOID,
        vec![(I64, "%walk".into()), (I64, "%parent".into())],
    );
    let blk = visitor.create_block("entry");
    for dep in &cross.module_init_deps {
        let target = blk.call(I64, &format!("{dep}__async_record"), &[]);
        blk.call_void(
            "js_async_module_visit",
            &[(I64, "%walk"), (I64, "%parent"), (I64, &target)],
        );
    }
    blk.ret_void();
    // Remaining direct init callers implement synchronous require. ESM
    // evaluation and import() enter through the record, never this wrapper.
    let wrapper = llmod.define_function(&format!("{prefix}__init"), VOID, vec![]);
    let blk = wrapper.create_block("entry");
    let addr = blk.call(I64, &format!("{prefix}__async_record"), &[]);
    blk.call_void("js_async_module_require", &[(I64, &addr)]);
    blk.ret_void();
    let entry = llmod.define_function(&format!("{prefix}__async_entry"), VOID, vec![]);
    let blk = entry.create_block("entry");
    let addr = blk.call(I64, &format!("{prefix}__async_record"), &[]);
    blk.call_void("js_async_module_entry", &[(I64, &addr)]);
    blk.ret_void();
}
