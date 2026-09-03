//! Bun runtime transpiler and in-memory build compatibility (#9602).
//!
//! This lives beside the TypeScript wrapper because both APIs need the same
//! pinned SWC generation.  Keeping it out of `perry-runtime` also means apps
//! which never ask for runtime compilation do not carry a bundler.

use std::collections::HashMap;
use std::ffi::c_void;
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once};

use anyhow::{anyhow, Context, Error};
use perry_ffi::{
    alloc_closure, alloc_null_proto_object, alloc_string, closure_capture_f64, drop_handle,
    gc_register_mutable_root_scanner_named, get_handle, iter_handles_of_mut, js_array_alloc,
    js_array_get, js_array_length, js_array_push, json_stringify, object_field_by_name,
    read_string, register_closure_arity, register_handle, set_closure_capture_f64, with_handle,
    with_handle_mut, ArrayHeader, GcRootVisitor, Handle, JsClosure, JsPromise, JsString, JsValue,
    Promise, RawClosureHeader, StringHeader, TransientRootScope,
};
use serde::{Deserialize, Serialize};
use swc_bundler::{
    BundleKind, Bundler, Config as BundleConfig, Hook, Load, ModuleData, ModuleRecord, ModuleType,
    Resolve,
};
use swc_common::{
    comments::SingleThreadedComments, sync::Lrc, FileName, Globals, Mark, SourceMap, Span, Spanned,
    GLOBALS,
};
use swc_ecma_ast::{
    Callee, Decl, EsVersion, ExportSpecifier, Expr, Lit, MemberProp, Module, ModuleDecl,
    ModuleExportName, ModuleItem, Pass, Pat, Program,
};
use swc_ecma_codegen::{text_writer::JsWriter, Config as CodegenConfig, Emitter};
use swc_ecma_loader::resolve::Resolution;
use swc_ecma_parser::{lexer::Lexer, EsSyntax, Parser, StringInput, Syntax, TsSyntax};
use swc_ecma_transforms_base::{helpers::Helpers, resolver};
use swc_ecma_transforms_react::{react, Options as ReactOptions, Runtime as ReactRuntime};
use swc_ecma_transforms_typescript::{tsx, typescript, Config as TypeScriptConfig, TsxConfig};
use swc_ecma_visit::{Visit, VisitWith};

const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const PLUGIN_FILE_PREFIX: &str = "perry-bun-plugin:";

extern "C" {
    fn js_get_string_pointer_unified(value: f64) -> *mut StringHeader;
    fn js_regexp_test(regexp: *const c_void, value: *const StringHeader) -> i32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BunLoader {
    Js,
    Jsx,
    Ts,
    Tsx,
}

impl BunLoader {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "js" | "mjs" | "cjs" => Ok(Self::Js),
            "jsx" => Ok(Self::Jsx),
            "ts" | "mts" | "cts" => Ok(Self::Ts),
            "tsx" => Ok(Self::Tsx),
            other => Err(format!(
                "Unsupported Bun loader '{other}'; expected js, jsx, ts, or tsx"
            )),
        }
    }

    fn for_path(path: &str) -> Self {
        let bare = path.split(['?', '#']).next().unwrap_or(path);
        match Path::new(bare)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "ts" | "mts" | "cts" => Self::Ts,
            "tsx" => Self::Tsx,
            "jsx" => Self::Jsx,
            _ => Self::Js,
        }
    }

    fn is_typescript(self) -> bool {
        matches!(self, Self::Ts | Self::Tsx)
    }

    fn has_jsx(self) -> bool {
        matches!(self, Self::Jsx | Self::Tsx)
    }
}

#[derive(Clone, Debug)]
struct BunDiagnostic {
    message: String,
    file: Option<String>,
    line: Option<u32>,
    column: Option<u32>,
}

impl BunDiagnostic {
    fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            file: None,
            line: None,
            column: None,
        }
    }

    fn parse(
        file: &str,
        source: &str,
        source_start: u32,
        span: Span,
        message: impl Into<String>,
    ) -> Self {
        let byte_offset = span.lo.0.saturating_sub(source_start) as usize;
        let prefix = source
            .get(..byte_offset.min(source.len()))
            .unwrap_or(source);
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
        let column = prefix
            .rsplit_once('\n')
            .map_or(prefix.len(), |(_, tail)| tail.len()) as u32
            + 1;
        Self {
            message: message.into(),
            file: Some(file.to_string()),
            line: Some(line),
            column: Some(column),
        }
    }

    fn render(&self) -> String {
        match (&self.file, self.line, self.column) {
            (Some(file), Some(line), Some(column)) => {
                format!("{file}:{line}:{column}: {}", self.message)
            }
            _ => self.message.clone(),
        }
    }
}

impl fmt::Display for BunDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.render())
    }
}

impl std::error::Error for BunDiagnostic {}

#[derive(Clone, Copy)]
struct BunTranspiler {
    loader: BunLoader,
    repl_mode: bool,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BunTranspilerOptions {
    loader: Option<String>,
    #[serde(default)]
    repl_mode: bool,
}

fn syntax_for(loader: BunLoader, repl_mode: bool) -> Syntax {
    if loader.is_typescript() {
        Syntax::Typescript(TsSyntax {
            tsx: loader.has_jsx(),
            decorators: true,
            dts: false,
            no_early_errors: false,
            disallow_ambiguous_jsx_like: false,
        })
    } else {
        Syntax::Es(EsSyntax {
            jsx: loader.has_jsx(),
            decorators: true,
            allow_return_outside_function: repl_mode,
            ..Default::default()
        })
    }
}

fn parse_bun_module(
    cm: &Lrc<SourceMap>,
    file_name: FileName,
    display_name: &str,
    source: &str,
    loader: BunLoader,
    repl_mode: bool,
    comments: Option<&SingleThreadedComments>,
) -> Result<(Lrc<swc_common::SourceFile>, Module), BunDiagnostic> {
    let source_file = cm.new_source_file(file_name.into(), source.to_string());
    let lexer = Lexer::new(
        syntax_for(loader, repl_mode),
        EsVersion::EsNext,
        StringInput::from(&*source_file),
        comments.map(|value| value as _),
    );
    let mut parser = Parser::new_from(lexer);
    let module = parser.parse_module().map_err(|error| {
        BunDiagnostic::parse(
            display_name,
            source,
            source_file.start_pos.0,
            error.span(),
            error.kind().msg().to_string(),
        )
    })?;
    if let Some(error) = parser.take_errors().into_iter().next() {
        return Err(BunDiagnostic::parse(
            display_name,
            source,
            source_file.start_pos.0,
            error.span(),
            error.kind().msg().to_string(),
        ));
    }
    Ok((source_file, module))
}

fn lower_bun_syntax(
    cm: &Lrc<SourceMap>,
    comments: &SingleThreadedComments,
    module: Module,
    loader: BunLoader,
) -> Module {
    let mut program = Program::Module(module);
    let unresolved_mark = Mark::new();
    let top_level_mark = Mark::new();
    resolver(unresolved_mark, top_level_mark, true).process(&mut program);
    if loader.is_typescript() {
        if loader.has_jsx() {
            tsx(
                cm.clone(),
                TypeScriptConfig::default(),
                TsxConfig::default(),
                comments.clone(),
                unresolved_mark,
                top_level_mark,
            )
            .process(&mut program);
        } else {
            typescript(TypeScriptConfig::default(), unresolved_mark, top_level_mark)
                .process(&mut program);
        }
    }
    if loader.has_jsx() {
        let mut options = ReactOptions {
            runtime: Some(ReactRuntime::Classic),
            ..Default::default()
        };
        options.next = Some(false);
        react(
            cm.clone(),
            Some(comments.clone()),
            options,
            top_level_mark,
            unresolved_mark,
        )
        .process(&mut program);
    }
    match program {
        Program::Module(module) => module,
        Program::Script(_) => unreachable!("Bun transpilation always parses a module"),
    }
}

fn emit_module(
    cm: Lrc<SourceMap>,
    comments: Option<&SingleThreadedComments>,
    module: &Module,
    minify: bool,
) -> Result<String, BunDiagnostic> {
    let mut output = Vec::new();
    let mut emitter = Emitter {
        cfg: CodegenConfig::default()
            .with_target(EsVersion::EsNext)
            .with_minify(minify),
        cm: cm.clone(),
        comments: comments.map(|value| value as _),
        wr: JsWriter::new(cm, "\n", &mut output, None),
    };
    emitter
        .emit_module(module)
        .map_err(|error| BunDiagnostic::message(format!("Failed to emit JavaScript: {error}")))?;
    String::from_utf8(output)
        .map_err(|error| BunDiagnostic::message(format!("Invalid emitted UTF-8: {error}")))
}

fn transform_bun_source(
    source: &str,
    file_name: &str,
    loader: BunLoader,
    repl_mode: bool,
) -> Result<String, BunDiagnostic> {
    let cm: Lrc<SourceMap> = Default::default();
    let comments = SingleThreadedComments::default();
    GLOBALS.set(&Globals::new(), || {
        let (_, module) = parse_bun_module(
            &cm,
            FileName::Custom(file_name.to_string()),
            file_name,
            source,
            loader,
            repl_mode,
            Some(&comments),
        )?;
        let module = lower_bun_syntax(&cm, &comments, module, loader);
        emit_module(cm.clone(), Some(&comments), &module, false)
    })
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct ScannedImport {
    path: String,
    kind: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct ScanResult {
    exports: Vec<String>,
    imports: Vec<ScannedImport>,
}

#[derive(Default)]
struct CallImportScanner {
    imports: Vec<(u32, ScannedImport)>,
}

impl CallImportScanner {
    fn string_argument(call: &swc_ecma_ast::CallExpr) -> Option<String> {
        let argument = call.args.first()?;
        if argument.spread.is_some() {
            return None;
        }
        match argument.expr.as_ref() {
            Expr::Lit(Lit::Str(value)) => Some(value.value.to_string_lossy().into_owned()),
            _ => None,
        }
    }
}

impl Visit for CallImportScanner {
    fn visit_call_expr(&mut self, call: &swc_ecma_ast::CallExpr) {
        let kind = match &call.callee {
            Callee::Import(_) => Some("dynamic-import"),
            Callee::Expr(callee) => match callee.as_ref() {
                Expr::Ident(ident) if ident.sym == *"require" => Some("require-call"),
                Expr::Member(member)
                    if matches!(member.obj.as_ref(), Expr::Ident(ident) if ident.sym == *"require")
                        && matches!(&member.prop, MemberProp::Ident(ident) if ident.sym == *"resolve") =>
                {
                    Some("require-resolve")
                }
                _ => None,
            },
            _ => None,
        };
        if let (Some(kind), Some(path)) = (kind, Self::string_argument(call)) {
            self.imports.push((
                call.span.lo.0,
                ScannedImport {
                    path,
                    kind: kind.to_string(),
                },
            ));
        }
        call.visit_children_with(self);
    }
}

fn export_name(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::Ident(ident) => ident.sym.to_string(),
        ModuleExportName::Str(value) => value.value.to_string_lossy().into_owned(),
    }
}

fn pattern_names(pattern: &Pat, output: &mut Vec<String>) {
    match pattern {
        Pat::Ident(ident) => output.push(ident.id.sym.to_string()),
        Pat::Array(array) => {
            for element in array.elems.iter().flatten() {
                pattern_names(element, output);
            }
        }
        Pat::Object(object) => {
            for property in &object.props {
                match property {
                    swc_ecma_ast::ObjectPatProp::KeyValue(value) => {
                        pattern_names(&value.value, output)
                    }
                    swc_ecma_ast::ObjectPatProp::Assign(value) => {
                        output.push(value.key.sym.to_string())
                    }
                    swc_ecma_ast::ObjectPatProp::Rest(value) => pattern_names(&value.arg, output),
                }
            }
        }
        Pat::Rest(rest) => pattern_names(&rest.arg, output),
        Pat::Assign(assign) => pattern_names(&assign.left, output),
        Pat::Invalid(_) | Pat::Expr(_) => {}
    }
}

fn declaration_exports(declaration: &Decl, output: &mut Vec<String>) {
    match declaration {
        Decl::Class(class) => output.push(class.ident.sym.to_string()),
        Decl::Fn(function) => output.push(function.ident.sym.to_string()),
        Decl::Var(variable) => {
            for declaration in &variable.decls {
                pattern_names(&declaration.name, output);
            }
        }
        _ => {}
    }
}

fn scan_bun_source(
    source: &str,
    file_name: &str,
    loader: BunLoader,
    repl_mode: bool,
) -> Result<ScanResult, BunDiagnostic> {
    let cm: Lrc<SourceMap> = Default::default();
    GLOBALS.set(&Globals::new(), || {
        let (_, module) = parse_bun_module(
            &cm,
            FileName::Custom(file_name.to_string()),
            file_name,
            source,
            loader,
            repl_mode,
            None,
        )?;
        let mut imports = Vec::new();
        let mut exports = Vec::new();
        for item in &module.body {
            let ModuleItem::ModuleDecl(declaration) = item else {
                continue;
            };
            match declaration {
                ModuleDecl::Import(import) if !import.type_only => imports.push((
                    import.span.lo.0,
                    ScannedImport {
                        path: import.src.value.to_string_lossy().into_owned(),
                        kind: "import-statement".to_string(),
                    },
                )),
                ModuleDecl::ExportDecl(export) => declaration_exports(&export.decl, &mut exports),
                ModuleDecl::ExportNamed(export) => {
                    if let Some(source) = &export.src {
                        imports.push((
                            export.span.lo.0,
                            ScannedImport {
                                path: source.value.to_string_lossy().into_owned(),
                                kind: "import-statement".to_string(),
                            },
                        ));
                    }
                    if !export.type_only {
                        for specifier in &export.specifiers {
                            match specifier {
                                ExportSpecifier::Named(named) if !named.is_type_only => exports
                                    .push(export_name(
                                        named.exported.as_ref().unwrap_or(&named.orig),
                                    )),
                                ExportSpecifier::Default(default) => {
                                    exports.push(default.exported.sym.to_string())
                                }
                                ExportSpecifier::Namespace(namespace) => {
                                    exports.push(export_name(&namespace.name))
                                }
                                _ => {}
                            }
                        }
                    }
                }
                ModuleDecl::ExportDefaultDecl(_) | ModuleDecl::ExportDefaultExpr(_) => {
                    exports.push("default".to_string())
                }
                ModuleDecl::ExportAll(export) if !export.type_only => imports.push((
                    export.span.lo.0,
                    ScannedImport {
                        path: export.src.value.to_string_lossy().into_owned(),
                        kind: "import-statement".to_string(),
                    },
                )),
                _ => {}
            }
        }
        let mut calls = CallImportScanner::default();
        module.visit_with(&mut calls);
        imports.extend(calls.imports);
        imports.sort_by_key(|(position, _)| *position);
        exports.dedup();
        Ok(ScanResult {
            exports,
            imports: imports.into_iter().map(|(_, value)| value).collect(),
        })
    })
}

fn read_header_string(pointer: *const StringHeader) -> String {
    if pointer.is_null() {
        return String::new();
    }
    unsafe { read_string(JsString::from_raw(pointer as *mut StringHeader)) }
        .map(str::to_owned)
        .unwrap_or_default()
}

fn transpiler_for(handle: Handle) -> Result<BunTranspiler, String> {
    get_handle::<BunTranspiler>(handle)
        .copied()
        .ok_or_else(|| "Invalid Bun.Transpiler receiver".to_string())
}

fn loader_override(pointer: *const StringHeader) -> Result<Option<BunLoader>, String> {
    if pointer.is_null() {
        return Ok(None);
    }
    BunLoader::parse(&read_header_string(pointer)).map(Some)
}

fn throw_transpiler_error(error: BunDiagnostic) -> ! {
    perry_ffi::throw_with_code(
        &error.render(),
        "BUN_TRANSPILE_ERROR",
        perry_ffi::ErrorKind::Error,
    )
}

/// Construct the native state behind `new Bun.Transpiler(options)`.
#[no_mangle]
pub extern "C" fn js_bun_transpiler_new(options: f64) -> Handle {
    let value = JsValue::from_bits(options.to_bits());
    let parsed = if value.is_undefined() || value.is_null() {
        BunTranspilerOptions::default()
    } else {
        let json = json_stringify(value).unwrap_or_default();
        serde_json::from_str::<BunTranspilerOptions>(&json).unwrap_or_else(|error| {
            perry_ffi::throw_with_code(
                &format!("Invalid Bun.Transpiler options: {error}"),
                "ERR_INVALID_ARG_VALUE",
                perry_ffi::ErrorKind::TypeError,
            )
        })
    };
    let loader = parsed
        .loader
        .as_deref()
        .map(BunLoader::parse)
        .transpose()
        .unwrap_or_else(|message| {
            perry_ffi::throw_with_code(
                &message,
                "ERR_INVALID_ARG_VALUE",
                perry_ffi::ErrorKind::TypeError,
            )
        })
        .unwrap_or(BunLoader::Js);
    register_handle(BunTranspiler {
        loader,
        repl_mode: parsed.repl_mode,
    })
}

/// `transpiler.transformSync(source, loader?)`.
#[no_mangle]
pub extern "C" fn js_bun_transpiler_transform_sync(
    handle: Handle,
    source: *const StringHeader,
    loader: *const StringHeader,
) -> *mut StringHeader {
    let transpiler = transpiler_for(handle).unwrap_or_else(|message| {
        perry_ffi::throw_with_code(
            &message,
            "ERR_INVALID_THIS",
            perry_ffi::ErrorKind::TypeError,
        )
    });
    let loader = loader_override(loader)
        .unwrap_or_else(|message| {
            perry_ffi::throw_with_code(
                &message,
                "ERR_INVALID_ARG_VALUE",
                perry_ffi::ErrorKind::TypeError,
            )
        })
        .unwrap_or(transpiler.loader);
    let source = read_header_string(source);
    match transform_bun_source(&source, "<transpiler>", loader, transpiler.repl_mode) {
        Ok(output) => alloc_string(&output).as_raw(),
        Err(error) => throw_transpiler_error(error),
    }
}

/// Promise-returning `transpiler.transform(source, loader?)`.
#[no_mangle]
pub extern "C" fn js_bun_transpiler_transform(
    handle: Handle,
    source: *const StringHeader,
    loader: *const StringHeader,
) -> *mut Promise {
    let result = transpiler_for(handle).and_then(|transpiler| {
        let loader = loader_override(loader)?.unwrap_or(transpiler.loader);
        let source = read_header_string(source);
        transform_bun_source(&source, "<transpiler>", loader, transpiler.repl_mode)
            .map_err(|error| error.render())
    });
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    match result {
        Ok(output) => promise.resolve_string(&output),
        Err(error) => promise.reject_string(&error),
    }
    raw
}

fn scan_for_handle(handle: Handle, source: *const StringHeader) -> Result<ScanResult, String> {
    let transpiler = transpiler_for(handle)?;
    scan_bun_source(
        &read_header_string(source),
        "<transpiler>",
        transpiler.loader,
        transpiler.repl_mode,
    )
    .map_err(|error| error.render())
}

/// `transpiler.scanImports(source)`; codegen parses the returned JSON.
#[no_mangle]
pub extern "C" fn js_bun_transpiler_scan_imports(
    handle: Handle,
    source: *const StringHeader,
) -> *mut StringHeader {
    let result = scan_for_handle(handle, source).unwrap_or_else(|message| {
        perry_ffi::throw_with_code(&message, "BUN_TRANSPILE_ERROR", perry_ffi::ErrorKind::Error)
    });
    let json = serde_json::to_string(&result.imports).unwrap_or_else(|_| "[]".to_string());
    alloc_string(&json).as_raw()
}

/// `transpiler.scan(source)`; codegen parses the returned JSON.
#[no_mangle]
pub extern "C" fn js_bun_transpiler_scan(
    handle: Handle,
    source: *const StringHeader,
) -> *mut StringHeader {
    let result = scan_for_handle(handle, source).unwrap_or_else(|message| {
        perry_ffi::throw_with_code(&message, "BUN_TRANSPILE_ERROR", perry_ffi::ErrorKind::Error)
    });
    let json = serde_json::to_string(&result)
        .unwrap_or_else(|_| "{\"exports\":[],\"imports\":[]}".to_string());
    alloc_string(&json).as_raw()
}

#[derive(Clone)]
struct PluginHook {
    filter: i64,
    callback: i64,
    namespace: Option<String>,
}

#[derive(Default)]
struct BuildHookSession {
    resolve: Vec<PluginHook>,
    load: Vec<PluginHook>,
}

static PLUGIN_GC_REGISTERED: Once = Once::new();
static NATIVE_CLOSURE_ARITIES_REGISTERED: Once = Once::new();

fn ensure_build_runtime_registered() {
    PLUGIN_GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named(
            "perry-ext-typescript:bun-build",
            scan_build_hook_roots,
        );
    });
    NATIVE_CLOSURE_ARITIES_REGISTERED.call_once(|| {
        register_closure_arity(bun_on_resolve as *const u8, 2);
        register_closure_arity(bun_on_load as *const u8, 2);
        register_closure_arity(bun_build_output_text as *const u8, 0);
    });
}

fn scan_build_hook_roots(visitor: &mut GcRootVisitor<'_>) {
    iter_handles_of_mut::<BuildHookSession, _>(|session| {
        for hook in session.resolve.iter_mut().chain(session.load.iter_mut()) {
            visitor.visit_i64_slot(&mut hook.filter);
            visitor.visit_i64_slot(&mut hook.callback);
        }
    });
}

fn raw_heap_address(value: f64) -> i64 {
    let bits = value.to_bits();
    if bits >> 48 == 0x7FFD {
        (bits & POINTER_MASK) as i64
    } else if bits >> 48 == 0 && bits >= 0x10000 {
        bits as i64
    } else {
        0
    }
}

fn string_value(value: JsValue) -> Option<String> {
    if !value.is_any_string() {
        return None;
    }
    let pointer = unsafe { js_get_string_pointer_unified(f64::from_bits(value.bits())) };
    if pointer.is_null() {
        return None;
    }
    unsafe { read_string(JsString::from_raw(pointer)) }.map(str::to_owned)
}

fn field(value: f64, name: &str) -> JsValue {
    object_field_by_name(JsValue::from_bits(value.to_bits()), name)
}

fn register_plugin_hook(
    closure: *const RawClosureHeader,
    options: f64,
    callback: f64,
    resolve_hook: bool,
) -> f64 {
    let session = unsafe { closure_capture_f64(closure, 0) } as Handle;
    let scope = TransientRootScope::enter();
    let options = scope.root_nanbox(options);
    let callback = scope.root_nanbox(callback);
    let filter = scope.root_nanbox(f64::from_bits(field(options.get(), "filter").bits()));
    let filter = raw_heap_address(filter.get());
    let callback = raw_heap_address(callback.get());
    if filter == 0 || callback == 0 {
        perry_ffi::throw_with_code(
            "Bun plugin hooks require a RegExp filter and callback",
            "ERR_INVALID_ARG_TYPE",
            perry_ffi::ErrorKind::TypeError,
        );
    }
    let namespace = string_value(field(options.get(), "namespace"));
    let hook = PluginHook {
        filter,
        callback,
        namespace,
    };
    let inserted = with_handle_mut::<BuildHookSession, _, _>(session, |hooks| {
        if resolve_hook {
            hooks.resolve.push(hook);
        } else {
            hooks.load.push(hook);
        }
    })
    .is_some();
    if !inserted {
        perry_ffi::throw_with_code(
            "Bun plugin setup used an expired build context",
            "ERR_INVALID_STATE",
            perry_ffi::ErrorKind::Error,
        );
    }
    f64::from_bits(JsValue::UNDEFINED.bits())
}

extern "C" fn bun_on_resolve(closure: *const RawClosureHeader, options: f64, callback: f64) -> f64 {
    register_plugin_hook(closure, options, callback, true)
}

extern "C" fn bun_on_load(closure: *const RawClosureHeader, options: f64, callback: f64) -> f64 {
    register_plugin_hook(closure, options, callback, false)
}

fn native_closure_value(function: *const u8, session: Handle) -> JsValue {
    let closure = alloc_closure(function, 1);
    if closure.is_null() {
        return JsValue::UNDEFINED;
    }
    unsafe { set_closure_capture_f64(closure, 0, session as f64) };
    JsValue::from_object_ptr(closure)
}

fn configure_plugins(options: f64) -> Handle {
    ensure_build_runtime_registered();
    let session = register_handle(BuildHookSession::default());
    let scope = TransientRootScope::enter();
    let options = scope.root_nanbox(options);
    let plugins = scope.root_nanbox(f64::from_bits(field(options.get(), "plugins").bits()));
    let array = JsValue::from_bits(plugins.get().to_bits()).as_pointer::<ArrayHeader>();
    if array.is_null() {
        return session;
    }
    let length = unsafe { js_array_length(array) };
    let mut callbacks = Vec::new();
    for index in 0..length {
        let array = JsValue::from_bits(plugins.get().to_bits()).as_pointer::<ArrayHeader>();
        let plugin =
            scope.root_nanbox(f64::from_bits(unsafe { js_array_get(array, index) }.bits()));
        let setup = field(plugin.get(), "setup");
        let pointer = raw_heap_address(f64::from_bits(setup.bits()));
        if pointer != 0 {
            callbacks.push(scope.root_addr(pointer));
        }
    }
    if callbacks.is_empty() {
        return session;
    }
    let on_resolve = scope.root_nanbox(f64::from_bits(
        native_closure_value(bun_on_resolve as *const u8, session).bits(),
    ));
    let on_load = scope.root_nanbox(f64::from_bits(
        native_closure_value(bun_on_load as *const u8, session).bits(),
    ));
    let builder = scope.root_nanbox(f64::from_bits(
        alloc_null_proto_object(&[
            ("onResolve", JsValue::from_bits(on_resolve.get().to_bits())),
            ("onLoad", JsValue::from_bits(on_load.get().to_bits())),
        ])
        .bits(),
    ));
    for callback in callbacks {
        let closure = unsafe { JsClosure::from_raw(callback.get() as *const RawClosureHeader) };
        unsafe { closure.call1(builder.get()) };
    }
    session
}

fn hook_matches(hook: &PluginHook, path: &str, namespace: &str) -> bool {
    if hook
        .namespace
        .as_deref()
        .is_some_and(|expected| expected != namespace)
    {
        return false;
    }
    let scope = TransientRootScope::enter();
    let filter = scope.root_addr(hook.filter);
    let path = alloc_string(path);
    unsafe { js_regexp_test(filter.get() as *const c_void, path.as_raw()) != 0 }
}

fn hooks_for(session: Handle, resolve: bool) -> Vec<PluginHook> {
    with_handle::<BuildHookSession, _, _>(session, |hooks| {
        if resolve {
            hooks.resolve.clone()
        } else {
            hooks.load.clone()
        }
    })
    .unwrap_or_default()
}

fn callback_args(fields: &[(&str, &str)]) -> f64 {
    let values = fields
        .iter()
        .map(|(name, value)| {
            (
                *name,
                JsValue::from_string_ptr(alloc_string(value).as_raw()),
            )
        })
        .collect::<Vec<_>>();
    f64::from_bits(alloc_null_proto_object(&values).bits())
}

fn invoke_hook(hook: &PluginHook, args: f64) -> JsValue {
    let scope = TransientRootScope::enter();
    let callback = scope.root_addr(hook.callback);
    let args = scope.root_nanbox(args);
    let closure = unsafe { JsClosure::from_raw(callback.get() as *const RawClosureHeader) };
    JsValue::from_bits(unsafe { closure.call1(args.get()) }.to_bits())
}

fn file_parts(file: &FileName) -> (String, String) {
    match file {
        FileName::Real(path) => ("file".to_string(), path.to_string_lossy().into_owned()),
        FileName::Custom(value) => {
            decode_plugin_file(value).unwrap_or_else(|| ("file".to_string(), value.to_string()))
        }
        other => ("file".to_string(), other.to_string()),
    }
}

fn encode_plugin_file(namespace: &str, path: &str) -> FileName {
    FileName::Custom(format!("{PLUGIN_FILE_PREFIX}{namespace}\u{1f}{path}"))
}

fn decode_plugin_file(value: &str) -> Option<(String, String)> {
    let value = value.strip_prefix(PLUGIN_FILE_PREFIX)?;
    let (namespace, path) = value.split_once('\u{1f}')?;
    Some((namespace.to_string(), path.to_string()))
}

struct BunResolver {
    session: Handle,
}

impl Resolve for BunResolver {
    fn resolve(&self, base: &FileName, specifier: &str) -> Result<Resolution, Error> {
        let (base_namespace, importer) = file_parts(base);
        for hook in hooks_for(self.session, true) {
            if !hook_matches(&hook, specifier, &base_namespace) {
                continue;
            }
            let resolve_dir = Path::new(&importer)
                .parent()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default();
            let args = callback_args(&[
                ("path", specifier),
                ("importer", &importer),
                ("namespace", &base_namespace),
                ("resolveDir", &resolve_dir),
                ("kind", "import-statement"),
            ]);
            let scope = TransientRootScope::enter();
            let result = scope.root_nanbox(f64::from_bits(invoke_hook(&hook, args).bits()));
            let result_value = result.get();
            let Some(path) = string_value(field(result_value, "path")) else {
                if JsValue::from_bits(result_value.to_bits()).is_undefined()
                    || JsValue::from_bits(result_value.to_bits()).is_null()
                {
                    continue;
                }
                return Err(anyhow!("Bun onResolve result is missing a string path"));
            };
            let namespace = string_value(field(result.get(), "namespace"))
                .unwrap_or_else(|| "file".to_string());
            let filename = if namespace == "file" {
                FileName::Real(PathBuf::from(path))
            } else {
                encode_plugin_file(&namespace, &path)
            };
            return Ok(Resolution {
                filename,
                slug: None,
            });
        }

        if base_namespace != "file" {
            return Err(anyhow!(
                "Could not resolve '{specifier}' from {base_namespace}:{importer}"
            ));
        }
        let base_dir = Path::new(&importer)
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let unresolved = Path::new(specifier);
        let candidate = if unresolved.is_absolute() {
            unresolved.to_path_buf()
        } else if specifier.starts_with('.') {
            base_dir.join(unresolved)
        } else {
            return Err(anyhow!(
                "Could not resolve package '{specifier}' from {importer}; mark it external or resolve it in a Bun plugin"
            ));
        };
        let candidates = if candidate.extension().is_some() {
            vec![candidate]
        } else {
            let mut values = vec![candidate.clone()];
            for extension in ["ts", "tsx", "js", "jsx", "mjs", "cjs"] {
                values.push(candidate.with_extension(extension));
            }
            for extension in ["ts", "tsx", "js", "jsx", "mjs", "cjs"] {
                values.push(candidate.join(format!("index.{extension}")));
            }
            values
        };
        let path = candidates
            .into_iter()
            .find(|path| path.is_file())
            .ok_or_else(|| anyhow!("Could not resolve '{specifier}' from {importer}"))?;
        Ok(Resolution {
            filename: FileName::Real(path),
            slug: None,
        })
    }
}

struct BunLoaderHost {
    cm: Lrc<SourceMap>,
    session: Handle,
    diagnostics: Arc<Mutex<Vec<BunDiagnostic>>>,
}

impl BunLoaderHost {
    fn plugin_load(
        &self,
        path: &str,
        namespace: &str,
    ) -> Result<Option<(String, BunLoader)>, Error> {
        for hook in hooks_for(self.session, false) {
            if !hook_matches(&hook, path, namespace) {
                continue;
            }
            let args = callback_args(&[("path", path), ("namespace", namespace)]);
            let scope = TransientRootScope::enter();
            let result = scope.root_nanbox(f64::from_bits(invoke_hook(&hook, args).bits()));
            let result_value = result.get();
            if JsValue::from_bits(result_value.to_bits()).is_undefined()
                || JsValue::from_bits(result_value.to_bits()).is_null()
            {
                continue;
            }
            let contents = string_value(field(result_value, "contents"))
                .ok_or_else(|| anyhow!("Bun onLoad result is missing string contents"))?;
            let loader = string_value(field(result.get(), "loader"))
                .map(|loader| BunLoader::parse(&loader))
                .transpose()
                .map_err(Error::msg)?
                .unwrap_or_else(|| BunLoader::for_path(path));
            return Ok(Some((contents, loader)));
        }
        Ok(None)
    }
}

impl Load for BunLoaderHost {
    fn load(&self, file: &FileName) -> Result<ModuleData, Error> {
        let (namespace, path) = file_parts(file);
        let (source, loader) = match self.plugin_load(&path, &namespace)? {
            Some(value) => value,
            None if namespace == "file" => {
                let source = std::fs::read_to_string(&path)
                    .with_context(|| format!("Could not load {path}"))?;
                (source, BunLoader::for_path(&path))
            }
            None => return Err(anyhow!("No Bun onLoad hook handled {namespace}:{path}")),
        };
        let comments = SingleThreadedComments::default();
        let parsed = parse_bun_module(
            &self.cm,
            file.clone(),
            &path,
            &source,
            loader,
            false,
            Some(&comments),
        );
        let (source_file, module) = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                self.diagnostics
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(error.clone());
                return Err(Error::new(error));
            }
        };
        let module = lower_bun_syntax(&self.cm, &comments, module, loader);
        Ok(ModuleData {
            fm: source_file,
            module,
            helpers: Helpers::new(false),
        })
    }
}

struct BunBundlerHook;

impl Hook for BunBundlerHook {
    fn get_import_meta_props(
        &self,
        _span: Span,
        _module_record: &ModuleRecord,
    ) -> Result<Vec<swc_ecma_ast::KeyValueProp>, Error> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct BunBuildOptions {
    entrypoints: Vec<PathBuf>,
    external: Vec<String>,
    minify: bool,
}

fn string_array(value: JsValue, field_name: &str) -> Result<Vec<String>, BunDiagnostic> {
    let scope = TransientRootScope::enter();
    let array_value = scope.root_nanbox(f64::from_bits(value.bits()));
    let array = JsValue::from_bits(array_value.get().to_bits()).as_pointer::<ArrayHeader>();
    if array.is_null() {
        return Err(BunDiagnostic::message(format!(
            "Bun.build {field_name} must be an array of strings"
        )));
    }
    let length = unsafe { js_array_length(array) };
    let mut values = Vec::with_capacity(length as usize);
    for index in 0..length {
        let array = JsValue::from_bits(array_value.get().to_bits()).as_pointer::<ArrayHeader>();
        let value = unsafe { js_array_get(array, index) };
        let value = scope.root_nanbox(f64::from_bits(value.bits()));
        let value = string_value(JsValue::from_bits(value.get().to_bits())).ok_or_else(|| {
            BunDiagnostic::message(format!("Bun.build {field_name}[{index}] must be a string"))
        })?;
        values.push(value);
    }
    Ok(values)
}

fn parse_build_options(options: f64) -> Result<BunBuildOptions, BunDiagnostic> {
    let value = JsValue::from_bits(options.to_bits());
    if !value.is_pointer_or_raw() {
        return Err(BunDiagnostic::message(
            "Bun.build options must be an object",
        ));
    }
    let entries = string_array(object_field_by_name(value, "entrypoints"), "entrypoints")?;
    if entries.is_empty() {
        return Err(BunDiagnostic::message(
            "Bun.build requires at least one entrypoint",
        ));
    }
    if let Some(target) = string_value(object_field_by_name(value, "target")) {
        if target != "bun" {
            return Err(BunDiagnostic::message(format!(
                "Bun.build target '{target}' is not supported; expected 'bun'"
            )));
        }
    }
    if let Some(format) = string_value(object_field_by_name(value, "format")) {
        if format != "esm" {
            return Err(BunDiagnostic::message(format!(
                "Bun.build format '{format}' is not supported; expected 'esm'"
            )));
        }
    }
    let external_value = object_field_by_name(value, "external");
    let external = if external_value.is_undefined() || external_value.is_null() {
        Vec::new()
    } else {
        string_array(external_value, "external")?
    };
    let minify = object_field_by_name(value, "minify");
    let minify = minify.is_bool() && minify.to_bool();
    let cwd = std::env::current_dir().map_err(|error| {
        BunDiagnostic::message(format!("Could not read current directory: {error}"))
    })?;
    Ok(BunBuildOptions {
        entrypoints: entries
            .into_iter()
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    cwd.join(path)
                }
            })
            .collect(),
        external,
        minify,
    })
}

struct BuildOutput {
    path: String,
    contents: String,
}

fn run_build(options_value: f64) -> Result<Vec<BuildOutput>, Vec<BunDiagnostic>> {
    let options = parse_build_options(options_value).map_err(|error| vec![error])?;
    let session = configure_plugins(options_value);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let cm: Lrc<SourceMap> = Default::default();
        let globals = Globals::new();
        let diagnostics = Arc::new(Mutex::new(Vec::<BunDiagnostic>::new()));
        GLOBALS.set(&globals, || {
            let mut bundler = Bundler::new(
                &globals,
                cm.clone(),
                BunLoaderHost {
                    cm: cm.clone(),
                    session,
                    diagnostics: diagnostics.clone(),
                },
                BunResolver { session },
                BundleConfig {
                    require: true,
                    disable_inliner: false,
                    disable_hygiene: false,
                    disable_fixer: false,
                    disable_dce: false,
                    external_modules: options
                        .external
                        .iter()
                        .map(|value| value.as_str().into())
                        .collect(),
                    module: ModuleType::Es,
                },
                Box::new(BunBundlerHook),
            );
            let entries = options
                .entrypoints
                .iter()
                .enumerate()
                .map(|(index, path)| (index.to_string(), FileName::Real(path.to_path_buf())))
                .collect::<HashMap<_, _>>();
            let bundles = bundler.bundle(entries).map_err(|error| {
                let diagnostic = diagnostics
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .first()
                    .cloned()
                    .or_else(|| {
                        error
                            .chain()
                            .find_map(|cause| cause.downcast_ref::<BunDiagnostic>())
                            .cloned()
                    })
                    .unwrap_or_else(|| {
                        BunDiagnostic::message(format!("Bun.build failed: {error:#}"))
                    });
                vec![diagnostic]
            })?;
            let mut outputs = Vec::new();
            for bundle in bundles {
                let name = match &bundle.kind {
                    BundleKind::Named { name } => name.clone(),
                    BundleKind::Dynamic => format!("dynamic-{}", outputs.len()),
                    BundleKind::Lib { name } => name.clone(),
                };
                let contents = emit_module(cm.clone(), None, &bundle.module, options.minify)
                    .map_err(|error| vec![error])?;
                let path = name
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| options.entrypoints.get(index))
                    .map(|path| path.with_extension("js").to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("{name}.js"));
                outputs.push(BuildOutput { path, contents });
            }
            outputs.sort_by(|left, right| left.path.cmp(&right.path));
            Ok(outputs)
        })
    }));
    drop_handle(session);
    match outcome {
        Ok(result) => result,
        Err(_) => Err(vec![BunDiagnostic::message(
            "Bun.build failed inside the bundler",
        )]),
    }
}

fn array_from_values(values: impl IntoIterator<Item = JsValue>) -> JsValue {
    let values = values.into_iter().collect::<Vec<_>>();
    let mut array = unsafe { js_array_alloc(values.len() as u32) };
    for value in values {
        array = unsafe { js_array_push(array, value) };
    }
    JsValue::from_object_ptr(array)
}

extern "C" fn bun_build_output_text(closure: *const RawClosureHeader) -> f64 {
    let scope = TransientRootScope::enter();
    let contents = scope.root_nanbox(unsafe { closure_capture_f64(closure, 0) });
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    promise.resolve(JsValue::from_bits(contents.get().to_bits()));
    f64::from_bits(JsValue::from_object_ptr(raw).bits())
}

fn build_output_value(output: BuildOutput) -> JsValue {
    ensure_build_runtime_registered();
    let scope = TransientRootScope::enter();
    let contents = scope.root_nanbox(f64::from_bits(
        JsValue::from_string_ptr(alloc_string(&output.contents).as_raw()).bits(),
    ));
    let text = alloc_closure(bun_build_output_text as *const u8, 1);
    unsafe { set_closure_capture_f64(text, 0, contents.get()) };
    let text = scope.root_nanbox(f64::from_bits(JsValue::from_object_ptr(text).bits()));
    let path = JsValue::from_string_ptr(alloc_string(&output.path).as_raw());
    alloc_null_proto_object(&[
        ("path", path),
        ("size", JsValue::from_number(output.contents.len() as f64)),
        (
            "kind",
            JsValue::from_string_ptr(alloc_string("entry-point").as_raw()),
        ),
        (
            "loader",
            JsValue::from_string_ptr(alloc_string("js").as_raw()),
        ),
        ("text", JsValue::from_bits(text.get().to_bits())),
    ])
}

fn diagnostic_value(diagnostic: BunDiagnostic) -> JsValue {
    let position = match (&diagnostic.file, diagnostic.line, diagnostic.column) {
        (Some(file), Some(line), Some(column)) => alloc_null_proto_object(&[
            (
                "file",
                JsValue::from_string_ptr(alloc_string(file).as_raw()),
            ),
            ("line", JsValue::from_number(line as f64)),
            ("column", JsValue::from_number(column as f64)),
        ]),
        _ => JsValue::NULL,
    };
    alloc_null_proto_object(&[
        (
            "message",
            JsValue::from_string_ptr(alloc_string(&diagnostic.message).as_raw()),
        ),
        (
            "level",
            JsValue::from_string_ptr(alloc_string("error").as_raw()),
        ),
        ("position", position),
    ])
}

fn build_result_value(result: Result<Vec<BuildOutput>, Vec<BunDiagnostic>>) -> JsValue {
    match result {
        Ok(outputs) => {
            let outputs = array_from_values(outputs.into_iter().map(build_output_value));
            alloc_null_proto_object(&[
                ("success", JsValue::TRUE),
                ("outputs", outputs),
                ("logs", array_from_values(std::iter::empty())),
            ])
        }
        Err(logs) => {
            let logs = array_from_values(logs.into_iter().map(diagnostic_value));
            alloc_null_proto_object(&[
                ("success", JsValue::FALSE),
                ("outputs", array_from_values(std::iter::empty())),
                ("logs", logs),
            ])
        }
    }
}

/// In-memory `Bun.build(options)` subset. The work is synchronous, but Bun's
/// public contract is a Promise so the completed result is wrapped here.
#[no_mangle]
pub extern "C" fn js_bun_build(options: f64) -> *mut Promise {
    let scope = TransientRootScope::enter();
    let options = scope.root_nanbox(options);
    let result = run_build(options.get());
    let value = scope.root_nanbox(f64::from_bits(build_result_value(result).bits()));
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    promise.resolve(JsValue::from_bits(value.get().to_bits()));
    raw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transpiler_erases_types() {
        let output = transform_bun_source(
            "const n: number = 1; export { n }",
            "fixture.ts",
            BunLoader::Ts,
            false,
        )
        .expect("transpile TypeScript");
        assert!(!output.contains(": number"));
        assert!(output.contains("const n = 1"));
        assert!(output.contains("export { n }"));
    }

    #[test]
    fn scan_reports_static_require_and_dynamic_imports_in_order() {
        let scan = scan_bun_source(
            "import x from 'pkg'; require('./cjs'); require.resolve('r'); import('./lazy'); export { x }",
            "fixture.js",
            BunLoader::Js,
            true,
        )
        .expect("scan JavaScript");
        assert_eq!(
            scan.imports,
            vec![
                ScannedImport {
                    path: "pkg".to_string(),
                    kind: "import-statement".to_string(),
                },
                ScannedImport {
                    path: "./cjs".to_string(),
                    kind: "require-call".to_string(),
                },
                ScannedImport {
                    path: "r".to_string(),
                    kind: "require-resolve".to_string(),
                },
                ScannedImport {
                    path: "./lazy".to_string(),
                    kind: "dynamic-import".to_string(),
                },
            ]
        );
        assert_eq!(scan.exports, vec!["x"]);
    }

    #[test]
    fn loader_names_match_bun_subset() {
        assert_eq!(BunLoader::parse("ts"), Ok(BunLoader::Ts));
        assert_eq!(BunLoader::parse("tsx"), Ok(BunLoader::Tsx));
        assert!(BunLoader::parse("css").is_err());
    }
}
