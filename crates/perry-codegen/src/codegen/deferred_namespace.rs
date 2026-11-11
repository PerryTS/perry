//! Cached deferred namespace identity and its existing-module-init evaluator.
use crate::module::LlModule;
use crate::types::{DOUBLE, I32, I64, I8, VOID};

pub(super) fn emit(llmod: &mut LlModule, prefix: &str, _is_entry: bool, async_deps: &[String]) {
    let ns = format!("@__perry_ns_{prefix}");
    let cached = format!("__perry_deferred_ns_{prefix}");
    let evaluator = format!("{prefix}__deferred_evaluate");
    llmod.add_module_state_global(&cached, DOUBLE, "0x7FFC000000000001");
    let f = llmod.define_function(&evaluator, DOUBLE, vec![]);
    f.create_block("entry");
    f.create_block("evaluate");
    f.create_block("not_ready");
    let ready = f.block_mut(1).unwrap().label.clone();
    let not_ready = f.block_mut(2).unwrap().label.clone();
    let b = f.block_mut(0).unwrap();
    let ready_value = b.call(I32, &format!("{prefix}__deferred_ready"), &[]);
    let blocked = b.icmp_eq(I32, &ready_value, "0");
    b.cond_br(&blocked, &not_ready, &ready);
    let b = f.block_mut(1).unwrap();
    b.call_void(&format!("{prefix}__init"), &[]);
    let value = b.load(DOUBLE, &ns);
    // Entry modules run in main; their namespace is not available mid-body.
    let bits = b.bitcast_double_to_i64(&value);
    let absent = b.icmp_eq(I64, &bits, crate::nanbox::TAG_UNDEFINED_I64);
    f.create_block("return");
    let ret = f.block_mut(3).unwrap().label.clone();
    f.block_mut(1).unwrap().cond_br(&absent, &not_ready, &ret);
    f.block_mut(3).unwrap().ret(DOUBLE, &value);
    let b = f.block_mut(2).unwrap();
    let error = b.call(DOUBLE, "js_deferred_namespace_not_ready", &[]);
    b.ret(DOUBLE, &error);

    for dep in async_deps {
        llmod.declare_function(&format!("{dep}__init"), VOID, &[]);
    }
    let f = llmod.define_function(&format!("{prefix}__deferred_namespace"), DOUBLE, vec![]);
    f.create_block("entry");
    f.create_block("create");
    f.create_block("return");
    let create = f.block_mut(1).unwrap().label.clone();
    let ret = f.block_mut(2).unwrap().label.clone();
    let global = format!("@{cached}");
    let b = f.block_mut(0).unwrap();
    // Reading the namespace value is not an evaluation: a deferred target's
    // asynchronous dependencies are gathered when the importing module runs,
    // not here, so this getter stays free of side effects.
    let current = b.load(DOUBLE, &global);
    let bits = b.bitcast_double_to_i64(&current);
    let absent = b.icmp_eq(I64, &bits, crate::nanbox::TAG_UNDEFINED_I64);
    b.cond_br(&absent, &create, &ret);
    let b = f.block_mut(1).unwrap();
    // The slot is registered once, when it first holds an object; a repeated
    // read of an already materialized namespace adds no root.
    let address = b.ptrtoint(&global, I64);
    b.call_void("js_gc_register_global_root", &[(I64, &address)]);
    let callback = format!("ptrtoint (ptr @{evaluator} to i64)");
    let value = b.call(DOUBLE, "js_create_deferred_namespace", &[(I64, &callback)]);
    crate::expr::emit_root_nanbox_store_on_block(b, &value, &global);
    b.br(&ret);
    let b = f.block_mut(2).unwrap();
    let value = b.load(DOUBLE, &global);
    b.ret(DOUBLE, &value);
    llmod.declare_function(&format!("{prefix}__init"), VOID, &[]);
}

/// Probe direct requested edges, pruning evaluated modules and cycle repeats.
pub(super) fn emit_readiness(
    llmod: &mut LlModule,
    prefix: &str,
    is_entry: bool,
    has_tla: bool,
    program_has_tla: bool,
    deps: &[String],
) {
    let guard = format!("__perry_init_done_{prefix}");
    if is_entry && !llmod.program_has_worker {
        llmod.add_internal_global(&guard, I8, "0");
    }
    for dep in deps {
        llmod.declare_function(&format!("{dep}__deferred_ready"), I32, &[]);
    }
    let scan = format!("{prefix}__deferred_deps_ready");
    let f = llmod.define_function(&scan, I32, vec![]);
    f.create_block("entry");
    let b = f.block_mut(0).unwrap();
    let mut ready = "1".to_string();
    for dep in deps {
        let value = b.call(I32, &format!("{dep}__deferred_ready"), &[]);
        ready = b.and(I32, &ready, &value);
    }
    b.ret(I32, &ready);
    let f = llmod.define_function(&format!("{prefix}__deferred_ready"), I32, vec![]);
    f.create_block("entry");
    let b = f.block_mut(0).unwrap();
    // A TLA program evaluates through the async record, so no module defines
    // the one-byte init guard; the runtime then has no state to read.
    let guard_addr = if program_has_tla {
        "0".to_string()
    } else {
        b.ptrtoint(&format!("@{guard}"), I64)
    };
    let callback = format!("ptrtoint (ptr @{scan} to i64)");
    let ready = b.call(
        I32,
        "js_deferred_module_ready",
        &[
            (I64, &guard_addr),
            (I64, &callback),
            (I32, if has_tla { "1" } else { "0" }),
        ],
    );
    b.ret(I32, &ready);
}
