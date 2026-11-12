use super::*;
use perry_hir::{Expr, Module, Stmt};

fn lowered(expr: Expr) -> RealmReach {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    let mut module = Module::new("exec");
    module.init.push(Stmt::Expr(expr));
    let mut reach = RealmReach::default();
    perry_hir::realm_reach::scan_lowered(&module, &index, &mut reach);
    reach
}

#[test]
fn realm_globalget_property_emits_exec_but_globalget_alone_does_not() {
    assert!(lowered(Expr::GlobalGet(0)).exec.is_empty());
    let r = lowered(Expr::PropertyGet {
        object: Box::new(Expr::GlobalGet(0)),
        property: "Buffer".into(),
        byte_offset: 0,
        byte_end: 0,
    });
    assert!(r.exec.contains("js_get_global_this"));
    assert!(r.whole.contains("Buffer"));
    assert!(r.names.contains("%Layout:ARRAY_PROTOTYPE_NAMED%"));
}

#[test]
fn realm_static_globalthis_does_not_own_global() {
    let r = lowered(Expr::PropertyGet {
        object: Box::new(Expr::GlobalThisExpr),
        property: "Buffer".into(),
        byte_offset: 0,
        byte_end: 0,
    });
    assert!(r.exec.contains("js_get_global_this"));
    assert!(r.whole.contains("Buffer"));
    assert!(!r.whole.contains("%Global%"));
}

#[test]
fn realm_bootstrap_exec_keeps_its_eager_array_layout() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    let mut r = RealmReach::default();
    r.exec("js_get_global_this");
    r.close(&index);
    assert!(r.names.contains("%Layout:ARRAY_PROTOTYPE_NAMED%"));
    assert!(!r.whole.contains("%Global%"));
}

#[test]
fn realm_dynamic_hook_write_resolves_without_global_alias() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    let mut r = RealmReach::default();
    assert_eq!(r.resolve(&index, "%Global%", "AsyncLocalStorage"), None);
    assert!(!index.has("AsyncLocalStorage"));
    r.global_read(&index, "AsyncLocalStorage");
    r.close(&index);
    assert!(!r.names.contains("node:async_hooks.AsyncLocalStorage"));
    r.exec("js_globalthis_seed_async_local_storage");
    r.close(&index);
    assert!(r.whole.contains("node:async_hooks.AsyncLocalStorage"));
    assert!(!r.whole.contains("%Global%"));
}

#[test]
fn realm_snapshot_roundtrip_retains_structural_partition() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    for own in [true, false] {
        let mut r = RealmReach::default();
        if own {
            r.own("console");
            r.own("console.log");
        } else {
            r.chain_roots.insert("Function".into());
        }
        r.close(&index);
        let snapshot = serde_json::to_value(&r).unwrap();
        for output in ["names", "whole", "structure", "top"] {
            assert!(snapshot.get(output).is_none());
        }
        let snapshot = serde_json::from_value(snapshot).unwrap();
        let restored = RealmReach::from_snapshot(snapshot, Default::default(), &index);
        assert_eq!(r, restored);
    }
}

#[test]
fn realm_descriptor_read_demand_is_not_a_global_birth() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    let mut r = RealmReach::default();
    r.exec("builtin_global_atom");
    r.close(&index);
    assert!(r.names.contains("%Layout:GLOBAL_OBJECT%"));
    assert!(!r.names.contains("%Global%"));
    assert!(!r.exec.contains("js_get_global_this"));
    assert!(!r.names.contains("%Layout:ARRAY_PROTOTYPE_NAMED%"));
    let once = r.clone();
    r.close(&index);
    assert_eq!(r, once);
}

#[test]
fn realm_descriptor_dumper_exec_adds_only_its_body_demands() {
    let r = lowered(Expr::ObjectGetOwnPropertyDescriptor(
        Box::new(Expr::GlobalGet(0)),
        Box::new(Expr::String("x".into())),
    ));
    assert!(r.exec.contains("js_object_get_own_property_descriptor"));
    for layout in [
        "%Layout:RECORD_F327_0%",
        "%Layout:RECORD_F328_0%",
        "%Layout:GLOBAL_OBJECT%",
    ] {
        assert!(r.names.contains(layout), "{layout}");
    }
    assert!(!r.names.contains("Object.getOwnPropertyDescriptor"));
    assert!(!r.names.contains("%Global%"));
}

#[test]
fn realm_literal_string_get_uses_the_emitted_operation() {
    let expression = Expr::PropertyGet {
        object: Box::new(Expr::String("x".into())),
        property: "constructor".into(),
        byte_offset: 0,
        byte_end: 0,
    };
    assert_eq!(
        perry_hir::realm_reach::lowered_operation(&expression),
        Some("js_object_get_field_by_name_f64")
    );
    let r = lowered(expression);
    assert!(r.exec.contains("js_object_get_field_by_name_f64"));
    assert!(r.names.contains("%Layout:GLOBAL_OBJECT%"));
    assert!(r.names.contains("%Layout:ARRAY_PROTOTYPE_NAMED%"));
    assert!(!r.names.contains("%Global%"));
}

#[test]
fn realm_native_member_exec_uses_the_declared_dispatch_body() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    assert_eq!(
        index.last("console.log").unwrap().body(),
        Some("js_console_log_spread")
    );
    let mut r = RealmReach::default();
    r.own("console");
    r.own("console.log");
    r.close(&index);
    assert!(r.executing(&index).contains("print_console_formatted"));
    assert!(r.structure.contains("%Layout:GLOBAL_OBJECT%"));
    assert!(!r.names.contains("%Global%"));
    for ctor in ["Object", "Function"] {
        assert!(!r.names.contains(ctor));
    }
}

#[test]
fn realm_stdio_value_reads_emit_their_allocating_owner_operation() {
    for (expr, body) in [
        (Expr::ProcessStdin, "js_process_stdin"),
        (Expr::ProcessStdout, "js_process_stdout"),
        (Expr::ProcessStderr, "js_process_stderr"),
    ] {
        assert_eq!(perry_hir::realm_reach::lowered_operation(&expr), Some(body));
        let r = lowered(expr);
        assert!(r.exec.contains(body), "{body}");
        assert!(
            r.structure.contains("%Layout:GLOBAL_OBJECT%"),
            "{body}: {r:?}"
        );
        assert!(!r.names.contains("%Global%"));
    }
}

#[test]
fn realm_actual_field_read_exec_survives_without_console() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    let mut module = crate::module::LlModule::new("x86_64-unknown-linux-gnu");
    let f = module.define_function("read", crate::types::DOUBLE, vec![]);
    let b = f.create_block("entry");
    b.call(
        crate::types::DOUBLE,
        perry_hir::realm_reach::field_get_operation(),
        &[(crate::types::I64, "0"), (crate::types::I64, "0")],
    );
    let exec = module.realm_exec(&index);
    assert!(!exec.is_empty());
    let r = RealmReach::from_snapshot(Default::default(), exec, &index);
    assert!(r.names.contains("%Layout:GLOBAL_OBJECT%"));
    assert!(!r.names.contains("%Global%"));
}

#[test]
fn realm_outlined_field_read_exec_covers_its_actual_slow_path() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    for symbol in [
        "js_object_get_field_value",
        "js_object_get_field_value_site",
    ] {
        let r = RealmReach::from_snapshot(Default::default(), [symbol.to_owned()].into(), &index);
        assert!(r.executing(&index).contains("get_field_miss"));
        assert!(r.names.contains("%Layout:GLOBAL_OBJECT%"));
        assert!(!r.names.contains("%Global%"));
    }
}

#[test]
fn realm_string_births_belong_to_the_allocating_members() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    for target in ["Array", "RegExp", "%StringIteratorPrototype%"] {
        assert!(
            !index.last("String").unwrap().births().any(|b| b == target),
            "{target}"
        );
    }
    for (owner, birth, prototype) in [
        ("String.prototype.split", "Array", "Array.prototype"),
        ("String.prototype.search", "RegExp", "RegExp.prototype"),
        (
            "String.prototype[\"@@iterator\"]",
            "%StringIteratorPrototype%",
            "%StringIteratorPrototype%",
        ),
    ] {
        assert!(index.last(owner).unwrap().births().any(|b| b == birth));
        let mut r = RealmReach::default();
        r.own(owner);
        r.close(&index);
        assert!(r.names.contains(prototype), "{owner} => {prototype}");
    }
}

#[test]
fn realm_regex_result_layouts_follow_the_executing_allocator() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    for layout in [
        "%Layout:REGEX_EXEC_NAMED%",
        "%Layout:REGEX_EXEC_INDICES_NAMED%",
        "%Layout:REGEX_INDICES_GROUPS_NAMED%",
    ] {
        assert!(
            !index.last("RegExp").unwrap().structure_has(layout),
            "{layout}"
        );
    }
    for symbol in ["js_string_search_js", "js_string_search_value"] {
        let r = RealmReach::from_snapshot(Default::default(), [symbol.to_owned()].into(), &index);
        assert!(r.names.contains("%Layout:REGEX_EXEC_NAMED%"), "{symbol}");
        assert!(r.names.contains("Array.prototype"));
        assert!(!r.names.contains("RegExp"));
    }
    for owner in ["String.prototype.search", "RegExp.prototype.exec"] {
        let mut r = RealmReach::default();
        r.own(owner);
        r.close(&index);
        assert!(r.names.contains("%Layout:REGEX_EXEC_NAMED%"));
    }
    let mut r = RealmReach::default();
    r.own("RegExp.prototype.test");
    r.close(&index);
    assert!(!r.names.contains("%Layout:REGEX_EXEC_NAMED%"));
}

#[test]
fn realm_runtime_value_reads_belong_to_the_subject_without_becoming_births() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    for kind in ["Date", "RegExp"] {
        let target = format!("{kind}.prototype");
        assert!(index
            .last(kind)
            .unwrap()
            .runtime_reads()
            .any(|v| v == target));
    }
    let mut r = RealmReach::default();
    r.own("console");
    r.own("console.log");
    r.close(&index);
    assert!(!r.names.contains("RegExp"));
    assert!(!r.names.contains("RegExp.prototype"));
    assert!(!r.names.contains("Date.prototype"));
}
