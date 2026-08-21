//! Native compatibility subset for TypeScript's runtime transpilation API.
//!
//! OpenCode Code Mode uses TypeScript only to erase TypeScript syntax from a
//! runtime string before feeding the emitted JavaScript to Acorn. Shipping the
//! upstream compiler for that narrow path is disproportionate, so this crate
//! exposes the audited `transpileModule` and `flattenDiagnosticMessageText`
//! surface using SWC, which Perry already uses for source parsing.

use perry_ffi::{alloc_string, json_stringify, read_string, JsString, JsValue, StringHeader};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use swc_common::{
    comments::SingleThreadedComments, sync::Lrc, FileName, Globals, Mark, SourceMap, Spanned,
    GLOBALS,
};
use swc_ecma_ast::{EsVersion, Pass, Program};
use swc_ecma_codegen::{text_writer::JsWriter, Config as CodegenConfig, Emitter};
use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax, TsSyntax};
use swc_ecma_transforms_base::resolver;
use swc_ecma_transforms_react::{react, Options as ReactOptions, Runtime as ReactRuntime};
use swc_ecma_transforms_typescript::{tsx, typescript, Config as TypeScriptConfig, TsxConfig};

const DIAGNOSTIC_ERROR: u8 = 1;
const SCRIPT_TARGET_ES_NEXT: i32 = 99;
const MODULE_KIND_ES_NEXT: i32 = 99;
const MODULE_KIND_PRESERVE: i32 = 200;
const JSX_PRESERVE: i32 = 1;
const JSX_REACT: i32 = 2;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranspileOptions {
    #[serde(default)]
    compiler_options: CompilerOptions,
    #[serde(default = "default_file_name")]
    file_name: String,
    #[serde(default)]
    report_diagnostics: bool,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompilerOptions {
    target: Option<i32>,
    module: Option<i32>,
    jsx: Option<i32>,
    jsx_factory: Option<String>,
    jsx_fragment_factory: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranspileOutput {
    output_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Vec<Diagnostic>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Diagnostic {
    category: u8,
    code: u32,
    message_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    start: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    length: Option<u32>,
}

impl Diagnostic {
    fn error(code: u32, message_text: impl Into<String>) -> Self {
        Self {
            category: DIAGNOSTIC_ERROR,
            code,
            message_text: message_text.into(),
            start: None,
            length: None,
        }
    }

    fn parse(code: u32, message_text: impl Into<String>, lo: u32, hi: u32) -> Self {
        // SWC byte positions are one-based. TypeScript diagnostics expose a
        // zero-based offset and a length.
        let start = lo.saturating_sub(1);
        Self {
            category: DIAGNOSTIC_ERROR,
            code,
            message_text: message_text.into(),
            start: Some(start),
            length: Some(hi.saturating_sub(lo).max(1)),
        }
    }
}

fn default_file_name() -> String {
    "module.ts".to_string()
}

fn options_from_json(json: Option<&str>) -> Result<TranspileOptions, Diagnostic> {
    match json {
        None | Some("") | Some("null") => Ok(TranspileOptions {
            file_name: default_file_name(),
            ..Default::default()
        }),
        Some(json) => serde_json::from_str(json).map_err(|error| {
            Diagnostic::error(90001, format!("Invalid transpile options: {error}"))
        }),
    }
}

fn validate_options(options: &TranspileOptions) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if !options.extra.is_empty() {
        let keys = options.extra.keys().cloned().collect::<Vec<_>>().join(", ");
        diagnostics.push(Diagnostic::error(
            90002,
            format!("Unsupported TypeScript transpile option(s): {keys}"),
        ));
    }
    if !options.compiler_options.extra.is_empty() {
        let keys = options
            .compiler_options
            .extra
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        diagnostics.push(Diagnostic::error(
            90003,
            format!("Unsupported TypeScript compiler option(s): {keys}"),
        ));
    }
    if let Some(target) = options.compiler_options.target {
        if target != SCRIPT_TARGET_ES_NEXT {
            diagnostics.push(Diagnostic::error(
                90004,
                format!(
                    "Unsupported ScriptTarget value {target}; Perry's native transpileModule subset supports ESNext"
                ),
            ));
        }
    }
    if let Some(module) = options.compiler_options.module {
        if !matches!(module, MODULE_KIND_ES_NEXT | MODULE_KIND_PRESERVE) {
            diagnostics.push(Diagnostic::error(
                90005,
                format!(
                    "Unsupported ModuleKind value {module}; Perry's native transpileModule subset supports ESNext and Preserve"
                ),
            ));
        }
    }
    if let Some(jsx) = options.compiler_options.jsx {
        if !matches!(jsx, JSX_PRESERVE | JSX_REACT) {
            diagnostics.push(Diagnostic::error(
                90006,
                format!(
                    "Unsupported JsxEmit value {jsx}; Perry's native transpileModule subset supports Preserve and React"
                ),
            ));
        }
    }

    diagnostics
}

fn is_tsx(file_name: &str) -> bool {
    let bare = file_name.split(['?', '#']).next().unwrap_or(file_name);
    bare.to_ascii_lowercase().ends_with(".tsx")
}

fn transpile(source: &str, options_json: Option<&str>) -> TranspileOutput {
    let options = match options_from_json(options_json) {
        Ok(options) => options,
        Err(error) => {
            return TranspileOutput {
                output_text: String::new(),
                diagnostics: Some(vec![error]),
            };
        }
    };
    let mut diagnostics = validate_options(&options);
    if !diagnostics.is_empty() {
        return TranspileOutput {
            output_text: String::new(),
            // Unsupported subset options are Perry compatibility errors, not
            // TypeScript syntax diagnostics. Keep them visible even when the
            // caller did not request ordinary parser diagnostics.
            diagnostics: Some(diagnostics),
        };
    }

    let cm: Lrc<SourceMap> = Default::default();
    let comments = SingleThreadedComments::default();
    let source_file = cm.new_source_file(
        Lrc::new(FileName::Custom(options.file_name.clone())),
        source.to_string(),
    );
    let tsx_enabled = is_tsx(&options.file_name);
    let lexer = Lexer::new(
        Syntax::Typescript(TsSyntax {
            tsx: tsx_enabled,
            decorators: true,
            dts: false,
            no_early_errors: false,
            disallow_ambiguous_jsx_like: false,
        }),
        EsVersion::EsNext,
        StringInput::from(&*source_file),
        Some(&comments),
    );
    let mut parser = Parser::new_from(lexer);
    let module = match parser.parse_module() {
        Ok(module) => module,
        Err(error) => {
            let span = error.span();
            diagnostics.push(Diagnostic::parse(
                1000,
                error.kind().msg().to_string(),
                span.lo.0,
                span.hi.0,
            ));
            return TranspileOutput {
                output_text: String::new(),
                diagnostics: options.report_diagnostics.then_some(diagnostics),
            };
        }
    };
    for error in parser.take_errors() {
        let span = error.span();
        diagnostics.push(Diagnostic::parse(
            1000,
            error.kind().msg().to_string(),
            span.lo.0,
            span.hi.0,
        ));
    }

    let mut program = Program::Module(module);
    GLOBALS.set(&Globals::new(), || {
        let unresolved_mark = Mark::new();
        let top_level_mark = Mark::new();
        resolver(unresolved_mark, top_level_mark, true).process(&mut program);
        if tsx_enabled {
            let tsx_config = TsxConfig {
                pragma: options.compiler_options.jsx_factory.clone().map(Into::into),
                pragma_frag: options
                    .compiler_options
                    .jsx_fragment_factory
                    .clone()
                    .map(Into::into),
            };
            tsx(
                cm.clone(),
                TypeScriptConfig::default(),
                tsx_config,
                comments.clone(),
                unresolved_mark,
                top_level_mark,
            )
            .process(&mut program);

            if options.compiler_options.jsx == Some(JSX_REACT) {
                let mut react_options = ReactOptions {
                    runtime: Some(ReactRuntime::Classic),
                    pragma: options.compiler_options.jsx_factory.clone().map(Into::into),
                    pragma_frag: options
                        .compiler_options
                        .jsx_fragment_factory
                        .clone()
                        .map(Into::into),
                    ..Default::default()
                };
                // Preserve the established classic-runtime behavior rather
                // than opting into Babel 8 defaults implicitly.
                react_options.next = Some(false);
                react(
                    cm.clone(),
                    Some(comments.clone()),
                    react_options,
                    top_level_mark,
                    unresolved_mark,
                )
                .process(&mut program);
            }
        } else {
            typescript(TypeScriptConfig::default(), unresolved_mark, top_level_mark)
                .process(&mut program);
        }
    });

    let mut output = Vec::new();
    let emit_result = {
        let mut emitter = Emitter {
            cfg: CodegenConfig::default().with_target(EsVersion::EsNext),
            cm: cm.clone(),
            comments: Some(&comments),
            wr: JsWriter::new(cm, "\n", &mut output, None),
        };
        emitter.emit_program(&program)
    };
    if let Err(error) = emit_result {
        diagnostics.push(Diagnostic::error(
            90007,
            format!("Failed to emit JavaScript: {error}"),
        ));
    }
    let output_text = String::from_utf8(output).unwrap_or_default();

    TranspileOutput {
        output_text,
        diagnostics: options.report_diagnostics.then_some(diagnostics),
    }
}

fn flatten_message(value: &Value, new_line: &str, indent: usize) -> String {
    match value {
        Value::String(message) => message.clone(),
        Value::Null => String::new(),
        Value::Object(object) => {
            let mut result = object
                .get("messageText")
                .map(|message| flatten_message(message, new_line, indent))
                .unwrap_or_default();
            if let Some(Value::Array(children)) = object.get("next") {
                for child in children {
                    if !result.is_empty() {
                        result.push_str(new_line);
                    }
                    result.push_str(&"  ".repeat(indent + 1));
                    result.push_str(&flatten_message(child, new_line, indent + 1));
                }
            }
            result
        }
        other => other.to_string(),
    }
}

/// `typescript.transpileModule(input, options)`.
///
/// Returns a JSON string which codegen converts back into the TypeScript
/// `{ outputText, diagnostics? }` object.
///
/// # Safety
///
/// `source_ptr` must be null or point to a Perry runtime string. `options` is
/// a NaN-boxed JavaScript value passed through an `f64` ABI slot.
#[no_mangle]
pub unsafe extern "C" fn js_typescript_transpile_module(
    source_ptr: *const StringHeader,
    options: f64,
) -> *mut StringHeader {
    let source = read_string(JsString::from_raw(source_ptr as *mut StringHeader))
        .map(str::to_owned)
        .unwrap_or_default();
    let options_json = json_stringify(JsValue::from_bits(options.to_bits()));
    let result = transpile(&source, options_json.as_deref());
    let json = serde_json::to_string(&result).unwrap_or_else(|error| {
        format!(
            "{{\"outputText\":\"\",\"diagnostics\":[{{\"category\":1,\"code\":90008,\"messageText\":{}}}]}}",
            serde_json::to_string(&error.to_string()).unwrap_or_else(|_| "\"serialization failed\"".to_string())
        )
    });
    alloc_string(&json).as_raw()
}

/// `typescript.flattenDiagnosticMessageText(messageText, newLine, indent?)`.
///
/// # Safety
///
/// `new_line_ptr` must be null or point to a Perry runtime string. `message`
/// is a NaN-boxed string or diagnostic-chain object.
#[no_mangle]
pub unsafe extern "C" fn js_typescript_flatten_diagnostic_message_text(
    message: f64,
    new_line_ptr: *const StringHeader,
    indent: f64,
) -> *mut StringHeader {
    let new_line = read_string(JsString::from_raw(new_line_ptr as *mut StringHeader))
        .map(str::to_owned)
        .unwrap_or_else(|| "\n".to_string());
    let value = json_stringify(JsValue::from_bits(message.to_bits()))
        .and_then(|json| serde_json::from_str::<Value>(&json).ok())
        .unwrap_or(Value::Null);
    let flattened = flatten_message(&value, &new_line, indent.max(0.0) as usize);
    alloc_string(&flattened).as_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opencode_options() -> &'static str {
        r#"{"reportDiagnostics":true,"compilerOptions":{"target":99,"module":99}}"#
    }

    #[test]
    fn erases_types_and_preserves_async_for_opencode() {
        let result = transpile(
            "async function __codemode__() { const value: number = await Promise.resolve(1); return value as number; }",
            Some(opencode_options()),
        );
        assert!(result.diagnostics.as_ref().is_some_and(Vec::is_empty));
        assert!(result.output_text.contains("async function __codemode__()"));
        assert!(result
            .output_text
            .contains("const value = await Promise.resolve(1)"));
        assert!(!result.output_text.contains(": number"));
        assert!(!result.output_text.contains("as number"));
    }

    #[test]
    fn preserves_es_module_imports() {
        let result = transpile(
            "import { value } from './dep.js'; export const answer: number = value;",
            Some(opencode_options()),
        );
        assert!(result
            .output_text
            .contains("import { value } from './dep.js'"));
        assert!(result.output_text.contains("export const answer = value"));
    }

    #[test]
    fn reports_parser_diagnostics() {
        let result = transpile("const value: = 1", Some(opencode_options()));
        let diagnostics = result.diagnostics.expect("diagnostics requested");
        assert!(!diagnostics.is_empty());
        assert_eq!(diagnostics[0].category, DIAGNOSTIC_ERROR);
        assert!(!diagnostics[0].message_text.is_empty());
    }

    #[test]
    fn filename_enables_tsx_and_react_lowering() {
        let result = transpile(
            "export const node: JSX.Element = <div id=\"x\">hi</div>;",
            Some(
                r#"{"fileName":"snippet.tsx","reportDiagnostics":true,"compilerOptions":{"target":99,"module":99,"jsx":2}}"#,
            ),
        );
        assert!(result.diagnostics.as_ref().is_some_and(Vec::is_empty));
        assert!(result.output_text.contains("React.createElement"));
        assert!(!result.output_text.contains("JSX.Element"));
    }

    #[test]
    fn unsupported_compiler_options_are_explicit() {
        let result = transpile(
            "const value: number = 1",
            Some(r#"{"reportDiagnostics":true,"compilerOptions":{"target":1,"module":1}}"#),
        );
        assert!(result.output_text.is_empty());
        let diagnostics = result.diagnostics.expect("diagnostics requested");
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics[0].message_text.contains("ScriptTarget"));
        assert!(diagnostics[1].message_text.contains("ModuleKind"));
    }

    #[test]
    fn compatibility_errors_do_not_depend_on_report_diagnostics() {
        let result = transpile(
            "const value: number = 1",
            Some(r#"{"compilerOptions":{"module":1}}"#),
        );
        let diagnostics = result.diagnostics.expect("compatibility diagnostic");
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message_text.contains("ModuleKind"));
    }

    #[test]
    fn flattens_diagnostic_message_chains() {
        let value = serde_json::json!({
            "messageText": "outer",
            "next": [{ "messageText": "inner" }]
        });
        assert_eq!(flatten_message(&value, "\n", 0), "outer\n  inner");
    }
}
