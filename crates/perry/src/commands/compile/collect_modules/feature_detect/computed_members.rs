//! Conservative capabilities for values whose member names are computed.
use perry_hir::ir::ImportSpecifier;
use perry_hir::{Expr, Function, Module, Stmt};

#[derive(Default)]
pub(super) struct Capabilities {
    pub opaque_members: bool,
    pub file_handles: bool,
    pub bun_files: bool,
    pub json_members: bool,
}

pub(super) fn scan(module: &Module) -> Capabilities {
    let open_names: Vec<&str> = module
        .imports
        .iter()
        .filter(|import| {
            !import.type_only
                && !import.runtime_erased
                && import.source.trim_start_matches("node:") == "fs/promises"
        })
        .flat_map(|import| &import.specifiers)
        .filter_map(|specifier| match specifier {
            ImportSpecifier::Named { imported, local } if imported == "open" => {
                Some([imported.as_str(), local.as_str()])
            }
            _ => None,
        })
        .flatten()
        .collect();
    let mut caps = Capabilities::default();
    for_each_expr(module, &mut |expr| {
        if let Expr::IndexGet { index, .. }
        | Expr::ReflectGet { key: index, .. }
        | Expr::ReflectGetOwnPropertyDescriptor { key: index, .. }
        | Expr::ObjectGetOwnPropertyDescriptor(_, index) = expr
        {
            caps.json_members |= matches!(index.as_ref(), Expr::String(name) if name == "json");
        }
        match expr {
            Expr::PropertyGet { property, .. } if matches!(property.as_str(), "TOML" | "JSONL") => {
                caps.json_members = true
            }
            Expr::NativeMethodCall {
                module,
                class_name,
                method,
                ..
            } if module.trim_start_matches("node:") == "bun"
                && (class_name
                    .as_deref()
                    .is_some_and(|name| matches!(name, "TOML" | "JSONL"))
                    || method.contains("toml")
                    || method.contains("TOML")
                    || method.contains("JSONL")) =>
            {
                caps.json_members = true
            }
            Expr::NativeMethodCall { module, method, .. }
                if module.trim_start_matches("node:") == "bun"
                    && matches!(method.as_str(), "file" | "stdin" | "stdout" | "stderr") =>
            {
                caps.bun_files = true
            }
            Expr::NativeModuleRef(module) if module.trim_start_matches("node:") == "bun" => {
                caps.bun_files = true
            }
            Expr::PropertyGet { property, .. } if property == "Bun" => caps.bun_files = true,
            // Numeric and literal keys do not hide a new API name. Literal
            // readableWebStream is already selected by the ordinary detector.
            Expr::IndexGet { index, .. } => caps.opaque_members |= opaque_key(index),
            Expr::ReflectGet { key, .. } | Expr::ReflectGetOwnPropertyDescriptor { key, .. } => {
                caps.opaque_members |= opaque_key(key);
            }
            Expr::ObjectGetOwnPropertyDescriptor(_, key) => caps.opaque_members |= opaque_key(key),
            Expr::ObjectGetOwnPropertyDescriptors(_)
            | Expr::ObjectValues(_)
            | Expr::ObjectEntries(_) => caps.opaque_members = true,
            Expr::ExternFuncRef { name, .. } if open_names.contains(&name.as_str()) => {
                caps.file_handles = true
            }
            Expr::NativeMethodCall { module, method, .. }
                if module.trim_start_matches("node:") == "fs/promises" && method == "open" =>
            {
                caps.file_handles = true
            }
            // An escaped namespace may supply open through another module.
            Expr::NativeModuleRef(module)
                if matches!(module.trim_start_matches("node:"), "fs" | "fs/promises") =>
            {
                caps.file_handles = true
            }
            Expr::PropertyGet { property, .. } if property == "getBuiltinModule" => {
                caps.file_handles = true
            }
            Expr::NativeMethodCall { method, .. } if method == "getBuiltinModule" => {
                caps.file_handles = true
            }
            _ => {}
        }
    });
    caps
}

fn opaque_key(key: &Expr) -> bool {
    !matches!(
        key,
        Expr::String(_)
            | Expr::Integer(_)
            | Expr::Number(_)
            | Expr::BigInt(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Undefined
    )
}

/// Visit every runtime expression, including lowered async closures and class
/// bodies. The caller accumulates facts across modules, so passing a native
/// object to a helper in another module cannot lose its capabilities.
pub(super) fn for_each_expr(module: &Module, visit: &mut impl FnMut(&Expr)) {
    fn expr_visit(expr: &Expr, visit: &mut impl FnMut(&Expr)) {
        visit(expr);
        if let Expr::Closure { params, body, .. } = expr {
            params_visit(params, visit);
            stmts(body, visit);
            return;
        }
        perry_hir::walker::walk_expr_children(expr, &mut |child| expr_visit(child, visit));
    }
    fn stmts(body: &[Stmt], visit: &mut impl FnMut(&Expr)) {
        for stmt in body {
            perry_hir::walker::stmt_any_expr(stmt, &mut |value| {
                expr_visit(value, visit);
                false
            });
        }
    }
    fn decorators(items: &[perry_hir::Decorator], visit: &mut impl FnMut(&Expr)) {
        for item in items {
            for arg in &item.args {
                expr_visit(arg, visit);
            }
        }
    }
    fn params_visit(params: &[perry_hir::Param], visit: &mut impl FnMut(&Expr)) {
        for param in params {
            if let Some(default) = &param.default {
                expr_visit(default, visit);
            }
            decorators(&param.decorators, visit);
        }
    }
    fn function(function: &Function, visit: &mut impl FnMut(&Expr)) {
        params_visit(&function.params, visit);
        stmts(&function.body, visit);
        decorators(&function.decorators, visit);
    }
    stmts(&module.init, visit);
    for value in &module.functions {
        function(value, visit);
    }
    for class in &module.classes {
        if let Some(parent) = &class.extends_expr {
            expr_visit(parent, visit);
        }
        for value in class
            .constructor
            .iter()
            .chain(&class.methods)
            .chain(&class.static_methods)
            .chain(class.getters.iter().map(|(_, value)| value))
            .chain(class.setters.iter().map(|(_, value)| value))
        {
            function(value, visit);
        }
        for field in class.fields.iter().chain(&class.static_fields) {
            if let Some(value) = &field.init {
                expr_visit(value, visit);
            }
            if let Some(key) = &field.key_expr {
                expr_visit(key, visit);
            }
            decorators(&field.decorators, visit);
        }
        for member in &class.computed_members {
            expr_visit(&member.key_expr, visit);
            function(&member.function, visit);
        }
        decorators(&class.decorators, visit);
    }
}

#[cfg(test)]
mod tests {
    use crate::commands::compile::CompilationContext;

    fn detect(ctx: &mut CompilationContext, source: &str, name: &str) {
        let ast = perry_parser::parse_typescript(source, name).expect("parse");
        let module = perry_hir::lower_module(&ast, name, name).expect("lower");
        super::super::detect_optional_feature_usage(ctx, &module);
    }
    fn context() -> CompilationContext {
        CompilationContext::new(std::path::PathBuf::from("/tmp"))
    }
    #[test]
    fn file_handle_computed_member_selects_streams() {
        for source in [
            r#"import {open} from "node:fs/promises"; async function f(key) { const file = await open("x", "r"); return file[key](); }"#,
            r#"import {open as start} from "node:fs/promises"; class C { async f(key) { const file = await start("x", "r"); return file[key](); } }"#,
        ] {
            let mut ctx = context();
            detect(&mut ctx, source, "entry.ts");
            assert!(ctx.native_module_imports.contains("stream/web"), "{source}");
        }
    }
    #[test]
    fn unrelated_indexing_does_not_make_a_file_read_use_streams() {
        let mut ctx = context();
        detect(
            &mut ctx,
            r#"import {readFile} from "node:fs/promises"; function f(names, i) { return readFile(names[i], "utf8"); }"#,
            "read.ts",
        );
        assert!(!ctx.native_module_imports.contains("stream/web"));
        let mut ctx = context();
        detect(
            &mut ctx,
            r#"import {open} from "node:fs/promises"; async function f() { const file = await open("x", "r"); return file.readFile("utf8"); }"#,
            "read.ts",
        );
        assert!(!ctx.native_module_imports.contains("stream/web"));
    }
    #[test]
    fn native_capability_and_computed_lookup_accumulate_in_either_module_order() {
        let sources = [
            r#"import {open} from "node:fs/promises"; export function make() { return open("x", "r"); }"#,
            "export function invoke(value, key) { return value[key](); }",
        ];
        for order in [[0, 1], [1, 0]] {
            let mut ctx = context();
            for i in order {
                detect(&mut ctx, sources[i], &format!("module{i}.ts"));
            }
            assert!(ctx.native_module_imports.contains("stream/web"));
        }
    }
    #[test]
    fn bun_file_computed_json_and_extracted_factory_select_json() {
        for source in [
            r#"function f(key) { return Bun.file("x")[key](); }"#,
            r#"const make = Bun.file; function f(key) { return make("x")[key](); }"#,
            r#"function f(key) { return Bun.stdin[key](); }"#,
            r#"Bun.file("x")["json"]();"#,
            r#"Bun.TOML.parse('answer = 42');"#,
            r#"Bun.JSONL.parseChunk('{"answer":42}\n');"#,
        ] {
            let mut ctx = context();
            detect(&mut ctx, source, "bun.ts");
            assert!(ctx.uses_json_engine, "{source}");
            assert!(!ctx.uses_global_json, "{source}");
        }
        for source in [
            r#"Bun.file(process.argv[2]).text();"#,
            r#"function f(values, index) { Bun.which("node"); return values[index]; }"#,
        ] {
            let mut ctx = context();
            detect(&mut ctx, source, "bun.ts");
            assert!(!ctx.uses_json_engine, "{source}");
            assert!(!ctx.uses_global_json, "{source}");
        }
    }
    #[test]
    fn bun_file_and_computed_lookup_accumulate_across_modules() {
        let sources = [
            "export function make() { return Bun.file('x'); }",
            "export function invoke(value, key) { return value[key](); }",
        ];
        for order in [[0, 1], [1, 0]] {
            let mut ctx = context();
            for i in order {
                detect(&mut ctx, sources[i], &format!("bun{i}.ts"));
            }
            assert!(ctx.uses_json_engine);
            assert!(!ctx.uses_global_json);
        }
    }
}
