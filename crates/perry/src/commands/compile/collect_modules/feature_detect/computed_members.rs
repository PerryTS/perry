//! Conservative capabilities for values whose member names are computed.
use perry_hir::analysis::{infer_expr_type, HirTypeEnv};
use perry_hir::ir::ImportSpecifier;
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Stmt};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(super) struct Capabilities {
    pub opaque_members: bool,
    pub date_members: bool,
    pub file_handles: bool,
    pub bun_files: bool,
    pub json_members: bool,
}

pub(super) fn scan(module: &Module, native_modules: &BTreeMap<PathBuf, Module>) -> Capabilities {
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
    let mut types = HirTypeEnv::from_module(module);
    refine_array_index_types(module, &mut types);
    refine_enum_image_types(module, &mut types);
    // `as const` can leave a declaration typed Any even though its immutable
    // initializer carries a closed literal shape. Reuse the existing binding
    // analysis, and only recover that shape when every read is a member read:
    // passing/storing the object or writing a property keeps it conservative.
    for (id, init) in perry_hir::dynamic_import::collect_module_const_locals(module) {
        if !matches!(types.local_type(id), Some(Type::Any | Type::Unknown)) {
            continue;
        }
        if let Expr::New { class_name, .. } = init {
            if module.classes.iter().any(|class| {
                class.name == *class_name
                    && class.is_literal_shape()
                    && class.fields.iter().all(|field| !api_value_type(&field.ty))
            }) && only_member_reads(module, id)
            {
                types.insert_local(id, Type::Named(class_name.clone()));
            }
        }
    }
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
            Expr::IndexGet { object, index, .. } => {
                caps.opaque_members |= opaque_key(index, &types)
                    && api_receiver(object, &types, module, native_modules);
                caps.date_members |= opaque_key(index, &types)
                    && date_receiver(object, &types, module, native_modules);
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
                    opaque_key(key, &types) && api_receiver(object, &types, module, native_modules);
                caps.date_members |= opaque_key(key, &types)
                    && date_receiver(object, &types, module, native_modules);
            }
            Expr::ObjectGetOwnPropertyDescriptor(object, key) => {
                caps.opaque_members |=
                    opaque_key(key, &types) && api_receiver(object, &types, module, native_modules);
                caps.date_members |= opaque_key(key, &types)
                    && date_receiver(object, &types, module, native_modules);
            }
            Expr::ObjectGetOwnPropertyDescriptors(object)
            | Expr::ObjectValues(object)
            | Expr::ObjectEntries(object) => {
                caps.opaque_members |= api_receiver(object, &types, module, native_modules);
                caps.date_members |= date_receiver(object, &types, module, native_modules);
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

// EnumImage is the existing lowering's proof that the IIFE births a
// scalar enum shape. Preserve that shape for its single assigned binding;
// escapes or later writes still prevent the passive-data proof.
fn refine_enum_image_types(module: &Module, types: &mut HirTypeEnv) {
    for_each_expr(module, &mut |expr| {
        let Expr::LocalSet(id, value) = expr else {
            return;
        };
        if !matches!(value.as_ref(), Expr::EnumImage { .. }) || !only_member_reads(module, *id) {
            return;
        }
        let mut writes = 0;
        for_each_expr(module, &mut |expr| {
            writes += usize::from(matches!(expr,
                Expr::LocalSet(local, _) | Expr::Update { id: local, .. } if local == id));
        });
        if writes == 1 {
            types.insert_local(*id, infer_expr_type(value, types));
        }
    });
}

fn exported_enum_image(producer: &Module, exported: &str) -> bool {
    let local = producer.exports.iter().find_map(|export| match export {
        perry_hir::ir::Export::Named {
            local,
            exported: name,
        } if name == exported => Some(local),
        _ => None,
    });
    let Some(local) = local else {
        return false;
    };
    let id = producer.init.iter().find_map(|stmt| match stmt {
        Stmt::Let { id, name, .. } if name == local => Some(*id),
        _ => None,
    });
    let Some(id) = id else {
        return false;
    };
    let mut types = HirTypeEnv::from_module(producer);
    refine_enum_image_types(producer, &mut types);
    matches!(types.local_type(id), Some(Type::Object(shape))
        if !shape.properties.is_empty() && shape.properties.values().all(|p| !api_value_type(&p.ty)))
}

fn date_receiver(
    object: &Expr,
    types: &HirTypeEnv,
    module: &Module,
    native_modules: &BTreeMap<PathBuf, Module>,
) -> bool {
    if !api_receiver(object, types, module, native_modules) {
        return false;
    }
    if matches!(
        object,
        Expr::Object(_) | Expr::ObjectSpread { .. } | Expr::EnumImage { .. }
    ) {
        return false;
    }
    // Imports use ExternFuncRef for values as well as functions. An unresolved
    // import can therefore be a Date despite its provisional function type.
    if matches!(object, Expr::ExternFuncRef { .. }) {
        return true;
    }
    fn returns_value(body: &[Stmt]) -> bool {
        body.iter().any(|stmt| match stmt {
            Stmt::Return(Some(_)) => true,
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => returns_value(then_branch) || else_branch.as_deref().is_some_and(returns_value),
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                returns_value(body)
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                returns_value(body)
                    || catch
                        .as_ref()
                        .is_some_and(|catch| returns_value(&catch.body))
                    || finally.as_deref().is_some_and(returns_value)
            }
            Stmt::Switch { cases, .. } => cases.iter().any(|case| returns_value(&case.body)),
            Stmt::Labeled { body, .. } => returns_value(std::slice::from_ref(body)),
            _ => false,
        })
    }
    fn may_be_date(ty: &Type, module: &Module, depth: usize) -> bool {
        match ty {
            Type::Any | Type::Unknown | Type::TypeVar(_) | Type::Object(_) => true,
            Type::Union(members) => members.iter().any(|ty| may_be_date(ty, module, depth)),
            Type::Named(name) | Type::Generic { base: name, .. } => {
                if name == "Date" || depth > module.classes.len() {
                    return true;
                }
                let Some(class) = module.classes.iter().find(|class| &class.name == name) else {
                    return true;
                };
                if class.is_literal_shape() {
                    return false;
                }
                // A constructor can replace its instance with another
                // object, including a Date. Its nominal name is then no proof.
                if class
                    .constructor
                    .as_ref()
                    .is_some_and(|ctor| returns_value(&ctor.body))
                {
                    return true;
                }
                if let Some(parent) = &class.extends_name {
                    return may_be_date(&Type::Named(parent.clone()), module, depth + 1);
                }
                if let Some(parent) = class
                    .extends
                    .and_then(|id| module.classes.iter().find(|c| c.id == id))
                {
                    return may_be_date(&Type::Named(parent.name.clone()), module, depth + 1);
                }
                class.extends_expr.is_some()
            }
            // Arrays remain arrays even when their elements can carry APIs.
            // A computed member read here cannot reach Date's prototype.
            _ => false,
        }
    }
    may_be_date(&infer_expr_type(object, types), module, 0)
}

// Native array iteration passes a numeric index even when an inline callback
// parameter was left Any. Recover only that call-shape fact. A reassigned
// parameter or rest parameter remains conservative. Restrict this to arrows:
// an ordinary function can call itself with a different index value.
fn refine_array_index_types(module: &Module, types: &mut HirTypeEnv) {
    for_each_expr(module, &mut |expr| {
        let (callback, position) = match expr {
            Expr::ArrayForEach { callback, .. }
            | Expr::ArrayMap { callback, .. }
            | Expr::ArrayFilter { callback, .. }
            | Expr::ArrayFind { callback, .. }
            | Expr::ArrayFindIndex { callback, .. }
            | Expr::ArrayFindLast { callback, .. }
            | Expr::ArrayFindLastIndex { callback, .. }
            | Expr::ArraySome { callback, .. }
            | Expr::ArrayEvery { callback, .. }
            | Expr::ArrayFlatMap { callback, .. } => (callback.as_ref(), 1),
            Expr::ArrayReduce { callback, .. } | Expr::ArrayReduceRight { callback, .. } => {
                (callback.as_ref(), 2)
            }
            _ => return,
        };
        let Expr::Closure {
            params, is_arrow, ..
        } = callback
        else {
            return;
        };
        let Some(index) = params.get(position) else {
            return;
        };
        if index.is_rest || !is_arrow {
            return;
        }
        let mut assigned = false;
        for_each_expr(module, &mut |expr| {
            assigned |= matches!(expr, Expr::LocalSet(id, _) if *id == index.id);
        });
        if !assigned {
            types.insert_local(index.id, Type::Number);
        }
    });
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
    fn may_name_api(ty: &Type) -> bool {
        match ty {
            Type::Number
            | Type::Int32
            | Type::BigInt
            | Type::Boolean
            | Type::Symbol
            | Type::Null
            | Type::Void
            | Type::Never => false,
            Type::Union(members) => members.iter().any(may_name_api),
            _ => true,
        }
    }
    if !may_name_api(&infer_expr_type(key, types)) {
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

fn only_member_reads(module: &Module, id: u32) -> bool {
    let mut reads = 0;
    let mut receivers = 0;
    for_each_expr(module, &mut |expr| {
        reads += usize::from(matches!(expr, Expr::LocalGet(local) if *local == id));
        // A method call can mutate its receiver; it is not a passive read.
        if let Expr::Call { callee, .. } | Expr::CallSpread { callee, .. } = expr {
            reads += usize::from(matches!(callee.as_ref(),
                Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. }
                if matches!(object.as_ref(), Expr::LocalGet(local) if *local == id)));
        }
        let object = match expr {
            Expr::IndexGet { object, .. }
            | Expr::PropertyGet { object, .. }
            | Expr::ReflectGet { target: object, .. }
            | Expr::ReflectGetOwnPropertyDescriptor { target: object, .. }
            | Expr::ObjectGetOwnPropertyDescriptor(object, _)
            | Expr::ObjectGetOwnPropertyDescriptors(object)
            | Expr::ObjectValues(object)
            | Expr::ObjectEntries(object) => Some(object.as_ref()),
            _ => None,
        };
        receivers += usize::from(matches!(object, Some(Expr::LocalGet(local)) if *local == id));
    });
    reads == receivers
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

fn api_receiver(
    object: &Expr,
    types: &HirTypeEnv,
    module: &Module,
    native_modules: &BTreeMap<PathBuf, Module>,
) -> bool {
    // Imported enums still look like external functions here. Their resolved
    // producer already carries the enum declaration used by fix_imported_enums
    // later in the pipeline; its members are scalar data, not native APIs.
    if let Expr::ExternFuncRef { name, .. } = object {
        if module.imports.iter().any(|import| {
            let Some(producer) = import
                .resolved_path
                .as_deref()
                .and_then(|path| native_modules.get(Path::new(path)))
            else {
                return false;
            };
            import.specifiers.iter().any(|specifier| {
                matches!(specifier, ImportSpecifier::Named { imported, local }
                    if local == name && (producer.enums.iter()
                        .any(|value| value.is_exported && value.name == *imported
                            && value.members.iter().all(|member| matches!(member.value,
                                perry_hir::EnumValue::Number(_) | perry_hir::EnumValue::String(_))))
                        || exported_enum_image(producer, imported)))
            })
        }) {
            return false;
        }
    }
    let ty = infer_expr_type(object, types);
    if let Type::Named(name) = &ty {
        // Lowering also represents closed object literals as classes. Reuse
        // their existing shape proof; an arbitrary named/native class is not
        // evidence that its dynamic members are only primitive data.
        if let Some(shape) = module.classes.iter().find(|class| class.name == *name) {
            if shape.is_literal_shape() {
                let closed = matches!(object, Expr::New { .. })
                    || matches!(object, Expr::LocalGet(id) if only_member_reads(module, *id));
                return !closed || shape.fields.iter().any(|field| api_value_type(&field.ty));
            }
        }
    }
    if api_value_type(&ty) {
        return true;
    }
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
        // Structural data types describe the initial fields, not later
        // Object.assign/defineProperty writes or mutations through an alias.
        // Only passive reads or an immediate literal preserve this proof.
        _ => match object {
            Expr::LocalGet(id) => !only_member_reads(module, *id),
            Expr::Object(_) | Expr::Array(_, _) | Expr::New { .. } | Expr::EnumImage { .. } => {
                false
            }
            _ => true,
        },
    }
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
            "class C { *[Symbol.iterator]() { const a: unknown[] = []; for (let i = a.length - 1; i >= 0; i--) yield a[i]; } }",
            "function f(k: string) { const order: Record<string, number> = { high: 0, low: 1 }; return order[k]; }",
            "function f(k: string) { const marks = { true: '[x]', false: '[ ]' } as const; return marks[k]; }",
            "function f(k: string) { const names = ['a', 'b']; return names[k]; }",
            "console.log(Object.values({ high: 0, low: 1 }));",
            "function f(a: unknown[], b: unknown[]) { return a.reduce((acc, v, i) => b[i], null); }",
            "function f(a: unknown[], b: unknown[]) { return a.reduceRight((acc, v, i) => b[i], null); }",
            "function f(a: unknown[], b: unknown[]) { return a.map((v, i) => b[i]); }",
            "function f(a: unknown[], b: unknown[]) { a.forEach((v, i) => console.log(b[i])); }",
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
            r#"import * as fs from "fs"; const data = { value: "x" } as const; Object.assign(data, fs); console.log(Object.values(data));"#,
            r#"import * as fs from "fs"; const data = { value: "x" }; Object.assign(data, fs); console.log(Object.values(data));"#,
        ] {
            let mut ctx = context();
            detect(&mut ctx, source, "escaped.ts");
            assert!(ctx.native_module_imports.contains("stream/web"), "{source}");
        }
    }

    #[test]
    fn reassigned_callback_index_can_still_name_an_api() {
        let mut ctx = context();
        detect(
            &mut ctx,
            r#"import {open} from "fs/promises"; async function f(a: unknown[], k: string) { const fh = await open("x", "r"); a.reduce((acc, v, i) => { i = k; return fh[i](); }, null); }"#,
            "callback.ts",
        );
        assert!(ctx.native_module_imports.contains("stream/web"));
    }

    #[test]
    fn resolved_imported_enum_values_do_not_select_temporal() {
        for (producer, expected) in [
            ("export enum Priority { High = 'high', Low = 'low' }", false),
            ("export function Priority() {}", true),
        ] {
            let mut ctx = context();
            let ast = perry_parser::parse_typescript(producer, "types.ts").expect("parse producer");
            let module =
                perry_hir::lower_module(&ast, "types.ts", "types.ts").expect("lower producer");
            ctx.native_modules
                .insert(std::path::PathBuf::from("/tmp/types.ts"), module);
            let source = "import {Priority as P} from './types'; console.log(new Date(0).getTime(), Object.values(P));";
            let ast = perry_parser::parse_typescript(source, "entry.ts").expect("parse consumer");
            let mut module =
                perry_hir::lower_module(&ast, "entry.ts", "entry.ts").expect("lower consumer");
            module.imports[0].resolved_path = Some("/tmp/types.ts".into());
            super::super::detect_optional_feature_usage(&mut ctx, &module);
            assert_eq!(ctx.uses_temporal, expected, "{producer}");
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
    fn detect_transformed(source: &str, name: &str) -> (CompilationContext, perry_hir::Module) {
        let ast = perry_parser::parse_typescript(source, name).expect("parse");
        let mut module = perry_hir::lower_module(&ast, name, name).expect("lower");
        perry_transform::transform_async_to_generator(&mut module);
        perry_transform::transform_generators(&mut module);
        let mut ctx = context();
        super::super::detect_optional_feature_usage(&mut ctx, &module);
        (ctx, module)
    }

    #[test]
    fn todo_esm_enum_does_not_select_temporal() {
        let mut ctx = context();
        let producer = r#"export var Priority;
            (function (Priority) {
                Priority["Low"] = "low"; Priority["High"] = "high";
            })(Priority || (Priority = {}));"#;
        let ast = perry_parser::parse_typescript(producer, "types.js").expect("parse producer");
        let module = perry_hir::lower_module(&ast, "types.js", "types.js").expect("lower producer");
        ctx.native_modules
            .insert(std::path::PathBuf::from("/tmp/types.js"), module);
        let source = "import {Priority} from './types'; console.log(new Date(0).getTime(), Object.values(Priority));";
        let ast = perry_parser::parse_typescript(source, "entry.js").expect("parse consumer");
        let mut module =
            perry_hir::lower_module(&ast, "entry.js", "entry.js").expect("lower consumer");
        module.imports[0].resolved_path = Some("/tmp/types.js".into());
        super::super::detect_optional_feature_usage(&mut ctx, &module);
        assert!(ctx.uses_date);
        assert!(!ctx.uses_temporal);
    }

    #[test]
    fn classes_generics_ts_does_not_select_temporal() {
        let (ctx, _) = detect_transformed(
            r#"
            class Stack<T> {
                #items: T[] = [];
                *[Symbol.iterator](): Iterator<T> {
                    for (let i = this.#items.length - 1; i >= 0; i--) yield this.#items[i];
                }
            }
            const tag = (strings: TemplateStringsArray, ...vals: unknown[]) =>
                strings.reduce((acc, s, i) => acc + s + (i < vals.length ? vals[i] : ""), "");
            console.log(new Date(0).getTime(), [...new Stack<string>()]);
        "#,
            "classes.ts",
        );
        assert!(ctx.uses_date);
        assert!(!ctx.uses_temporal);
    }

    #[test]
    fn classes_generics_esm_does_not_select_temporal() {
        let (ctx, _) = detect_transformed(
            r#"
            class Stack {
                #items = [];
                *[Symbol.iterator]() {
                    for (let i = this.#items.length - 1; i >= 0; i--) yield this.#items[i];
                }
            }
            const tag = (strings, ...vals) =>
                strings.reduce((acc, s, i) => acc + s + (i < vals.length ? vals[i] : ""), "");
            console.log(new Date(0).getTime(), [...new Stack()]);
        "#,
            "classes.js",
        );
        assert!(ctx.uses_date);
        assert!(!ctx.uses_temporal);
    }

    #[test]
    fn date_string_key_still_selects_temporal() {
        for source in [
            "function f(d: Date, k: string) { return d[k](); } console.log(new Date(0).getTime());",
            "const m = 'toTemporal' + 'Instant'; new Date()[m]();",
            "function f(d: {toISOString(): string}, k: string) { return d[k](); } new Date(0);",
            "class C { constructor() { return new Date(0); } } function f(k: string) { return new C()[k](); }",
            "function f(k: string, ...values) { values = new Date(0); return values[k](); }",
            "function f(d, k: string) { return d[k](); } console.log(new Date(0).getTime());",
        ] {
            let (ctx, _) = detect_transformed(source, "date.ts");
            assert!(ctx.uses_date, "{source}");
            assert!(ctx.uses_temporal, "{source}");
        }
    }

    #[test]
    fn enum_iife_keys_and_members_are_not_opaque() {
        for source in [
            r#"var E; (function(E) { E[E["a"] = 0] = "a"; E[E["b"] = 1] = "b"; })(E || (E = {}));
               function f(k: string) { return E[k]; } console.log(Object.values(E));"#,
            r#"var E; (function(E) { E["a"] = "a"; E["b"] = "b"; })(E || (E = {}));
               function f(k: string) { return E[k]; } console.log(Object.values(E));"#,
        ] {
            let (_, module) = detect_transformed(source, "enum.js");
            assert!(
                !super::scan(&module, &Default::default()).opaque_members,
                "{source}"
            );
        }
    }

    #[test]
    fn known_array_receiver_with_string_key_does_not_select_temporal() {
        let (ctx, module) = detect_transformed(
            "function f(a: unknown[], k: string) { return a[k]; } console.log(new Date(0).getTime());",
            "array.ts");
        assert!(super::scan(&module, &Default::default()).opaque_members);
        assert!(ctx.uses_date);
        assert!(!ctx.uses_temporal);
    }

    #[test]
    fn date_capability_and_unknown_receiver_accumulate_in_either_order() {
        for order in [[0, 1], [1, 0]] {
            let mut ctx = context();
            let sources = [
                "export const d = new Date(0);",
                "export function invoke(d, key: string) { return d[key](); }",
            ];
            for i in order {
                detect(&mut ctx, sources[i], &format!("date{i}.ts"));
            }
            assert!(ctx.uses_temporal);
        }
    }

    #[test]
    fn private_generator_numeric_key_does_not_select_diagnostics() {
        let (ctx, module) = detect_transformed(
            "class C { #items = []; *[Symbol.iterator]() { for (let i = this.#items.length - 1; i >= 0; i--) yield this.#items[i]; } } console.log(process.argv.length, new Date(0).getTime());",
            "generator.ts");
        assert!(!super::scan(&module, &Default::default()).opaque_members);
        assert!(!ctx.uses_diagnostics);
        assert!(!ctx.uses_temporal);
    }

    #[test]
    fn escaped_or_reassigned_enum_image_remains_opaque() {
        for use_value in ["Object.assign(E, source);", "mutate(E);", "E = source;"] {
            let source = format!(
                r#"var E; (function(E) {{ E[E["a"] = 0] = "a"; }})(E || (E = {{}}));
                function f(source) {{ {use_value} return Object.values(E); }}"#
            );
            let (_, module) = detect_transformed(&source, "enum.js");
            assert!(
                super::scan(&module, &Default::default()).opaque_members,
                "{source}"
            );
        }
    }
}
