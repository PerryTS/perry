//! Conservative capabilities for values whose member names are computed.
use perry_hir::analysis::{infer_expr_type, HirTypeEnv};
use perry_hir::ir::ImportSpecifier;
use perry_hir::types::Type;
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
    let types = HirTypeEnv::from_module(module);
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
            Expr::IndexGet { object, index } => {
                caps.opaque_members |=
                    opaque_key(index, &types) && api_receiver(object, &types, module);
            }
            Expr::ReflectGet {
                target: object,
                key,
                ..
            }
            | Expr::ReflectGetOwnPropertyDescriptor {
                target: object,
                key,
            } => {
                caps.opaque_members |=
                    opaque_key(key, &types) && api_receiver(object, &types, module);
            }
            Expr::ObjectGetOwnPropertyDescriptor(object, key) => {
                caps.opaque_members |=
                    opaque_key(key, &types) && api_receiver(object, &types, module);
            }
            Expr::ObjectGetOwnPropertyDescriptors(object)
            | Expr::ObjectValues(object)
            | Expr::ObjectEntries(object) => {
                caps.opaque_members |= api_receiver(object, &types, module)
            }
            Expr::ExternFuncRef { name, .. } if open_names.contains(&name.as_str()) => {
                caps.file_handles = true
            }
            Expr::NativeMethodCall { module, method, .. }
                if module.trim_start_matches("node:") == "fs/promises" && method == "open" =>
            {
                caps.file_handles = true
            }
            Expr::PropertyGet {
                object, property, ..
            } if property == "open"
                && matches!(object.as_ref(), Expr::NativeModuleRef(module)
                        if module.trim_start_matches("node:") == "fs/promises") =>
            {
                caps.file_handles = true
            }
            // An escaped namespace may supply open through another module.
            // The visitor excludes a namespace used only as a direct callee
            // receiver, while still visiting every argument as a value.
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

fn opaque_key(key: &Expr, types: &HirTypeEnv) -> bool {
    // Array IteratorRecord lowering wraps its Number cursor in an identity
    // marker. Read the cursor's existing type through that compiler-only
    // wrapper, just as codegen reads its operand.
    let key = match key {
        Expr::NativeMethodCall {
            module,
            class_name: None,
            object: None,
            method,
            args,
        } if module == "__perry_runtime" && method == "arrayRecordIndex" && args.len() == 1 => {
            &args[0]
        }
        _ => key,
    };
    // ToPropertyKey of a number produces a numeric spelling, never the name
    // of a FileHandle, Date or process API (including NaN and infinities).
    if matches!(infer_expr_type(key, types), Type::Number | Type::Int32) {
        return false;
    }
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

// Primitive data cannot supply a native namespace or method. Keep unknown,
// function and native-instance types conservative, including containers of
// such values; Object.values(fs) and arrays of FileHandles still carry APIs.
fn api_value_type(ty: &Type) -> bool {
    match ty {
        Type::Number
        | Type::Int32
        | Type::BigInt
        | Type::Boolean
        | Type::String
        | Type::StringLiteral(_)
        | Type::Symbol
        | Type::Null
        | Type::Void
        | Type::Never => false,
        Type::Array(element) => api_value_type(element),
        Type::Tuple(elements) | Type::Union(elements) => elements.iter().any(api_value_type),
        Type::Object(object)
            if !object.properties.is_empty() || object.index_signature.is_some() =>
        {
            object
                .properties
                .values()
                .any(|property| api_value_type(&property.ty))
                || object
                    .index_signature
                    .as_deref()
                    .is_some_and(api_value_type)
        }
        Type::Generic { base, type_args } if base == "Record" && type_args.len() == 2 => {
            api_value_type(&type_args[1])
        }
        _ => true,
    }
}

fn api_receiver(object: &Expr, types: &HirTypeEnv, module: &Module) -> bool {
    let ty = infer_expr_type(object, types);
    if let Type::Named(name) = &ty {
        // Lowering also represents closed object literals as classes. Reuse
        // their existing shape proof; an arbitrary named/native class is not
        // evidence that its dynamic members are only primitive data.
        if let Some(shape) = module.classes.iter().find(|class| class.name == *name) {
            if shape.is_literal_shape() {
                return shape.fields.iter().any(|field| api_value_type(&field.ty));
            }
        }
    }
    api_value_type(&ty)
}

fn is_fs_namespace(expr: &Expr) -> bool {
    matches!(expr, Expr::NativeModuleRef(module)
        if matches!(module.trim_start_matches("node:"), "fs" | "fs/promises"))
}

/// Visit runtime values, including lowered async closures and class
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
        // A statically named namespace call does not pass the namespace as
        // a value. Keep the member itself (notably fs/promises.open), and
        // recurse into arguments: fs.readFileSync(helper(fs), "utf8") escapes.
        if let Expr::Call { callee, .. } | Expr::CallSpread { callee, .. } = expr {
            if matches!(callee.as_ref(), Expr::PropertyGet { object, .. } if is_fs_namespace(object))
            {
                perry_hir::walker::walk_expr_children(expr, &mut |child| {
                    if std::ptr::eq(child, callee.as_ref()) {
                        visit(child);
                    } else {
                        expr_visit(child, visit);
                    }
                });
                return;
            }
        }
        if let Expr::NativeMethodCall {
            object: Some(object),
            args,
            ..
        } = expr
        {
            if is_fs_namespace(object) {
                for arg in args {
                    expr_visit(arg, visit);
                }
                return;
            }
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
    #[test]
    fn fs_reads_and_for_of_do_not_select_streams() {
        for source in [
            r#"import * as fs from "fs"; for (const x of ["a"]) console.log(fs.readFileSync(x, "utf8"));"#,
            r#"import * as fs from "fs"; console.log(fs.readFileSync("a", "utf8"));"#,
            r#"import * as fs from "fs"; for (const x of ["a"]) console.log(fs.existsSync(x));"#,
            r#"import * as fs from "fs"; for (const x of process.argv.slice(2)) console.log(fs.readFileSync(x, "utf8"));"#,
        ] {
            let mut ctx = context();
            detect(&mut ctx, source, "read.ts");
            assert!(
                !ctx.native_module_imports.contains("stream/web"),
                "{source}"
            );
        }
    }

    #[test]
    fn numeric_keys_do_not_select_temporal_or_diagnostics() {
        for lookup in [
            "for (const x of [1, 2]) console.log(x);",
            "for (const x of process.argv.slice(2)) console.log(x);",
            "function f(a, i: number) { return a[i]; }",
            "function f(a) { for (let i = 0; i < 2; i++) console.log(a[i]); }",
            "function f(a, i: number) { return Reflect.get(a, i); }",
            "function f(a, i: number) { return Object.getOwnPropertyDescriptor(a, i); }",
            "const f = (a, i: number) => a[i];",
            "class C { f(a, i: number) { return a[i]; } }",
            "function f(k: string) { const order: Record<string, number> = { high: 0, low: 1 }; return order[k]; }",
            "function f(k: string) { const marks = { true: '[x]', false: '[ ]' } as const; return marks[k]; }",
            "function f(k: string) { const names = ['a', 'b']; return names[k]; }",
            "console.log(Object.values({ high: 0, low: 1 }));",
        ] {
            let mut ctx = context();
            let source = format!("console.log(new Date(0).getTime(), process.argv.length); {lookup}");
            detect(&mut ctx, &source, "numeric.ts");
            assert!(ctx.uses_date, "{source}");
            assert!(!ctx.uses_temporal, "{source}");
            assert!(!ctx.uses_diagnostics, "{source}");
        }
    }

    #[test]
    fn direct_fs_call_receiver_is_not_an_escaped_namespace() {
        // The unrelated string lookup must not combine with an ordinary read
        // to manufacture a FileHandle capability. This isolates the receiver
        // rule from the numeric-key rule above.
        for source in [
            r#"import * as fs from "fs"; function f(path, object, key: string) { fs.readFileSync(path, "utf8"); return object[key]; }"#,
            r#"import * as fs from "node:fs"; function f(path, object, key: string) { fs.readFileSync(path, "utf8"); return object[key]; }"#,
            r#"import * as fs from "fs"; function f(args, object, key: string) { fs.readFileSync(...args); return object[key]; }"#,
        ] {
            let mut ctx = context();
            detect(&mut ctx, source, "direct.ts");
            assert!(!ctx.uses_file_handle_capability, "{source}");
            assert!(
                !ctx.native_module_imports.contains("stream/web"),
                "{source}"
            );
        }
    }

    #[test]
    fn string_keyed_file_handle_and_escaped_fs_keep_streams() {
        for source in [
            r#"import {open} from "node:fs/promises"; async function f(k: string) { const fh = await open("a", "r"); return fh[k](); }"#,
            r#"import * as fs from "fs"; function call(ns, k: string) { return ns[k](); } call(fs, process.argv[2]);"#,
            r#"import * as fs from "fs"; console.log(Object.values(fs));"#,
            r#"import * as fs from "fs"; const ns = fs; function f(k: string) { return ns[k](); }"#,
            r#"import * as fs from "fs"; function give() { return fs; } function f(k: string) { return give()[k](); }"#,
            r#"import * as fs from "fs"; fs[process.argv[2]]();"#,
            r#"const fs = process.getBuiltinModule("fs/promises"); function f(k: string) { return fs[k](); }"#,
            r#"import * as fs from "fs"; fs.readFileSync(pass(fs), "utf8"); function pass(ns) { return Object.values(ns)[0]; }"#,
        ] {
            let mut ctx = context();
            detect(&mut ctx, source, "escaped.ts");
            assert!(ctx.native_module_imports.contains("stream/web"), "{source}");
        }
    }

    #[test]
    fn namespace_passed_to_another_module_keeps_streams() {
        let sources = [
            r#"import * as fs from "fs"; import {invoke} from "./helper"; invoke(fs);"#,
            "export function invoke(ns) { return Object.values(ns); }",
        ];
        for order in [[0, 1], [1, 0]] {
            let mut ctx = context();
            for i in order {
                detect(&mut ctx, sources[i], &format!("module{i}.ts"));
            }
            assert!(ctx.native_module_imports.contains("stream/web"));
        }
    }
}
