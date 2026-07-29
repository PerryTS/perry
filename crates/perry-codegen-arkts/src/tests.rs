// Test module mechanically split out of lib.rs (issue #1100). Declared
// in lib.rs as `#[cfg(test)] mod tests;` so `use super::*;` keeps
// resolving to the crate root. Pure code move; no logic changes.
//
// Further split into topical sibling modules (chore: split large files).
// The shared test helpers stay here in the trunk and are re-exported to
// the siblings via `use super::*;` (siblings inherit this module's scope);
// the trunk's own `use super::*;` reaches the crate root, and the helpers
// below are `pub(crate)` so the siblings can call them.

use super::*;

// Topical sibling test modules. The whole subtree is already under
// `#[cfg(test)]` via the parent `mod tests;`, so siblings need no extra
// attribute.
mod charts_tree;
mod conditions;
mod containers;
mod mutations;
mod widgets;

pub(crate) fn empty_module() -> Module {
    Module {
        name: "test".to_string(),
        imports: vec![],
        exports: vec![],
        classes: vec![],
        interfaces: vec![],
        type_aliases: vec![],
        enums: vec![],
        globals: vec![],
        functions: vec![],
        script_global_functions: vec![],
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        init: vec![],
        exported_native_instances: vec![],
        exported_func_return_native_instances: vec![],
        exported_objects: vec![],
        exported_functions: vec![],
        widgets: vec![],
        uses_fetch: false,
        uses_webassembly: false,
        init_was_unrolled: false,
        extern_funcs: vec![],
        has_top_level_await: false,
        init_kind: perry_hir::ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        class_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

pub(crate) fn nmc(method: &str, args: Vec<Expr>) -> Expr {
    Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: method.to_string(),
        args,
    }
}

pub(crate) fn app_with_body(body: Expr) -> Stmt {
    Stmt::Expr(Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: "App".to_string(),
        args: vec![Expr::Object(vec![("body".to_string(), body)])],
    })
}

pub(crate) fn closure_stub() -> Expr {
    Expr::Closure {
        func_id: 0 as perry_hir::types::FuncId,
        params: vec![],
        return_type: perry_hir::types::Type::Any,
        body: vec![],
        captures: vec![],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        is_strict: false,
    }
}

// ----- Phase 2 v6: state<T> reactive container helpers -----

pub(crate) fn state_call(initial: Expr) -> Expr {
    Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: "state".to_string(),
        args: vec![initial],
    }
}

pub(crate) fn state_method_call(state_id: u32, method: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::LocalGet(state_id)),
            property: method.to_string(),
        }),
        args,
        type_args: vec![],
        byte_offset: 0,
    }
}

// ─── #369 perry/media drain glue helper ────────────────────────────

pub(crate) fn media_call(method: &str, args: Vec<Expr>) -> Expr {
    Expr::NativeMethodCall {
        module: "perry/media".to_string(),
        class_name: None,
        object: None,
        method: method.to_string(),
        args,
    }
}

// ─── #408 procedural mutation tracking helpers ─────────────────────

/// Helper: Let-bind a widget to a LocalId so mutator calls can target it.
pub(crate) fn let_widget(id: LocalId, name: &str, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: perry_hir::types::Type::Any,
        mutable: false,
        init: Some(init),
    }
}

/// Helper: a perry/ui mutator call expression, e.g. widgetAddChild(parent, child).
pub(crate) fn mutator_stmt(method: &str, args: Vec<Expr>) -> Stmt {
    Stmt::Expr(Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: method.to_string(),
        args,
    })
}

/// Helper: declare-const stmt for `__platform__` (the canonical HIR
/// shape `Stmt::Let { name, init: None }` — the same shape
/// `crates/perry-codegen/src/codegen.rs::compile_time_constants`
/// recognizes).
pub(crate) fn declare_const(id: LocalId, name: &str) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: perry_hir::types::Type::Any,
        mutable: false,
        init: None,
    }
}

/// Walk the source line-by-line and assert no line opens a `/*` that
/// contains a second `*/` after the first one (which would break
/// parsing). This is a tighter form of "no `*/` inside `/* ... */`":
/// for every block-comment marker, count the number of `*/` between
/// `/*` and the next `*/` — must be exactly one.
pub(crate) fn assert_no_nested_block_comments(src: &str) {
    let mut i = 0;
    let bytes = src.as_bytes();
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Found an opening `/*`. Find the matching close.
            let start = i;
            i += 2;
            let mut close = None;
            while i + 1 < bytes.len() {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    close = Some(i);
                    break;
                }
                i += 1;
            }
            let Some(close) = close else { return };
            // The comment body is bytes[start+2..close]. It must NOT
            // itself contain a `*/` (which would mean the original
            // close was actually the *second* close — impossible per
            // the inner-loop logic above, but the symmetric check
            // catches the other failure mode where serialize_condition
            // smuggled in a `*/` that was treated as the close.
            let body = &src[start + 2..close];
            assert!(
                !body.contains("*/"),
                "nested block comment found at {}: body={:?}\nfull source:\n{}",
                start,
                body,
                src
            );
            i = close + 2;
        } else {
            i += 1;
        }
    }
}
