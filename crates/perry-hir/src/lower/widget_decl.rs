//! AST to HIR lowering — extracted from `lower/mod.rs` (issue #1101).
//!
//! Pure mechanical split: no logic changes. Helpers keep their original
//! visibility and are re-exported from `lower/mod.rs` so the existing
//! `expr_*` submodules and the rest of the crate keep compiling unchanged.

use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;

mod reactive_animate;
pub(crate) use reactive_animate::try_desugar_reactive_animate;

/// Try to lower a Widget({...}) call from perry/widget into a WidgetDecl.
/// Returns Some(WidgetDecl) if this is a widget declaration, None otherwise.
pub(crate) fn try_lower_widget_decl(
    ctx: &LoweringContext,
    call_expr: &ast::CallExpr,
) -> Option<WidgetDecl> {
    // Check callee is a function imported from perry/widget named "Widget"
    let callee = match &call_expr.callee {
        ast::Callee::Expr(expr) => expr,
        _ => return None,
    };
    let func_name = match callee.as_ref() {
        ast::Expr::Ident(ident) => ident.sym.as_ref(),
        _ => return None,
    };
    let (module, method) = ctx.lookup_native_module(func_name)?;
    if module != "perry/widget" {
        return None;
    }
    let method_name = method.unwrap_or(func_name);
    if method_name != "Widget" {
        return None;
    }

    // First arg should be the config object literal
    let config_obj = match call_expr.args.first() {
        Some(arg) => match arg.expr.as_ref() {
            ast::Expr::Object(obj) => obj,
            _ => return None,
        },
        None => return None,
    };

    let mut kind = String::new();
    let mut display_name = String::new();
    let mut description = String::new();
    let mut supported_families: Vec<String> = Vec::new();
    let mut entry_fields: Vec<(String, WidgetFieldType)> = Vec::new();
    let mut render_body: Vec<WidgetNode> = Vec::new();
    let mut entry_param_name = "entry".to_string();
    let mut config_params: Vec<WidgetConfigParam> = Vec::new();
    let mut provider_func_name: Option<String> = None;
    let mut placeholder: Option<Vec<(String, WidgetPlaceholderValue)>> = None;
    let mut family_param_name: Option<String> = None;
    let mut app_group: Option<String> = None;
    // Compile-time `reloadPolicy: { after: { minutes: N } }` literals found in
    // the provider's return statements (converted to seconds), plus a flag for
    // `reloadPolicy` values that exist but aren't statically parseable.
    let mut reload_policy_seconds: Vec<u32> = Vec::new();
    let mut reload_policy_unparsed = false;

    for prop in &config_obj.props {
        let kv = match prop {
            ast::PropOrSpread::Prop(p) => match p.as_ref() {
                ast::Prop::KeyValue(kv) => kv,
                ast::Prop::Method(method) => {
                    let key = prop_name_to_string(&method.key);
                    if key == "render" {
                        // Extract parameter name
                        if let Some(param) = method.function.params.first() {
                            if let ast::Pat::Ident(ident) = &param.pat {
                                entry_param_name = ident.id.sym.to_string();
                            }
                        }
                        // Check for 2nd parameter (family)
                        if let Some(param) = method.function.params.get(1) {
                            if let ast::Pat::Ident(ident) = &param.pat {
                                family_param_name = Some(ident.id.sym.to_string());
                            }
                        }
                        // Extract type annotation for entry fields (only if not already specified via entryFields)
                        if entry_fields.is_empty() {
                            if let Some(param) = method.function.params.first() {
                                extract_entry_fields_from_param(&param.pat, &mut entry_fields);
                            }
                        }
                        // Parse render body — detect family switches
                        if let Some(body) = &method.function.body {
                            let nodes = parse_render_body_stmts(&body.stmts, &family_param_name);
                            render_body = nodes;
                        }
                    } else if key == "provider" {
                        // Provider as method: provider(config) { ... }
                        let func_name = format!("__widget_provider_{}", kind);
                        provider_func_name = Some(func_name);
                        if let Some(body) = &method.function.body {
                            scan_provider_stmts_for_reload_policy(
                                &body.stmts,
                                &mut reload_policy_seconds,
                                &mut reload_policy_unparsed,
                            );
                        }
                    }
                    continue;
                }
                _ => continue,
            },
            _ => continue,
        };

        let key = prop_name_to_string(&kv.key);
        match key.as_str() {
            "kind" => {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                    kind = s.value.as_str().unwrap_or("").to_string();
                }
            }
            "displayName" => {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                    display_name = s.value.as_str().unwrap_or("").to_string();
                }
            }
            "description" => {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                    description = s.value.as_str().unwrap_or("").to_string();
                }
            }
            "supportedFamilies" => {
                if let ast::Expr::Array(arr) = kv.value.as_ref() {
                    for ast::ExprOrSpread { expr, .. } in arr.elems.iter().flatten() {
                        if let ast::Expr::Lit(ast::Lit::Str(s)) = expr.as_ref() {
                            supported_families.push(s.value.as_str().unwrap_or("").to_string());
                        }
                    }
                }
            }
            "appGroup" => {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                    app_group = Some(s.value.as_str().unwrap_or("").to_string());
                }
            }
            "config" => {
                // Parse config object → Vec<WidgetConfigParam>
                if let ast::Expr::Object(obj) = kv.value.as_ref() {
                    for field_prop in &obj.props {
                        if let ast::PropOrSpread::Prop(p) = field_prop {
                            if let ast::Prop::KeyValue(field_kv) = p.as_ref() {
                                let param_name = prop_name_to_string(&field_kv.key);
                                if let Some(param) =
                                    parse_widget_config_param(&param_name, &field_kv.value)
                                {
                                    config_params.push(param);
                                }
                            }
                        }
                    }
                }
            }
            "provider" => {
                // Arrow function provider: provider: async (config) => { ... }
                if let ast::Expr::Arrow(arrow) = kv.value.as_ref() {
                    let func_name = if kind.is_empty() {
                        "__widget_provider_widget".to_string()
                    } else {
                        let safe = kind.rsplit('.').next().unwrap_or(&kind);
                        format!("__widget_provider_{}", safe)
                    };
                    provider_func_name = Some(func_name);
                    match arrow.body.as_ref() {
                        ast::BlockStmtOrExpr::BlockStmt(block) => {
                            scan_provider_stmts_for_reload_policy(
                                &block.stmts,
                                &mut reload_policy_seconds,
                                &mut reload_policy_unparsed,
                            );
                        }
                        ast::BlockStmtOrExpr::Expr(expr) => {
                            scan_provider_return_expr_for_reload_policy(
                                expr,
                                &mut reload_policy_seconds,
                                &mut reload_policy_unparsed,
                            );
                        }
                    }
                }
            }
            "placeholder" => {
                if let ast::Expr::Object(obj) = kv.value.as_ref() {
                    let mut fields = Vec::new();
                    for field_prop in &obj.props {
                        if let ast::PropOrSpread::Prop(p) = field_prop {
                            if let ast::Prop::KeyValue(field_kv) = p.as_ref() {
                                let field_name = prop_name_to_string(&field_kv.key);
                                let val = parse_placeholder_value(&field_kv.value);
                                fields.push((field_name, val));
                            }
                        }
                    }
                    placeholder = Some(fields);
                }
            }
            "entryFields" => {
                // Allow explicit entry field declarations. Issue #1179
                // follow-up: parse_entry_field_value_spec recurses through
                // array literals (`[X]` → `Array<X>`) and object literals
                // (`{k: 'string'}` → `Object`), so users can describe
                // nested shapes without falling into the catch-all that
                // used to collapse everything to `String` and break
                // SwiftUI `ForEach` over arrays of objects.
                if let ast::Expr::Object(obj) = kv.value.as_ref() {
                    for field_prop in &obj.props {
                        if let ast::PropOrSpread::Prop(p) = field_prop {
                            if let ast::Prop::KeyValue(field_kv) = p.as_ref() {
                                let field_name = prop_name_to_string(&field_kv.key);
                                let field_type =
                                    parse_entry_field_value_spec(field_kv.value.as_ref());
                                entry_fields.push((field_name, field_type));
                            }
                        }
                    }
                }
            }
            "render" => {
                // Arrow function: render: (entry) => VStack(...)
                if let ast::Expr::Arrow(arrow) = kv.value.as_ref() {
                    // Extract parameter name
                    if let Some(param) = arrow.params.first() {
                        if let ast::Pat::Ident(ident) = param {
                            entry_param_name = ident.id.sym.to_string();
                        }
                    }
                    // Check for 2nd parameter (family)
                    if let Some(param) = arrow.params.get(1) {
                        if let ast::Pat::Ident(ident) = param {
                            family_param_name = Some(ident.id.sym.to_string());
                        }
                    }
                    // Extract entry fields from type annotation (only if not already specified via entryFields)
                    if entry_fields.is_empty() {
                        if let Some(param) = arrow.params.first() {
                            extract_entry_fields_from_param(param, &mut entry_fields);
                        }
                    }
                    // Parse body
                    match arrow.body.as_ref() {
                        ast::BlockStmtOrExpr::Expr(expr) => {
                            if let Some(node) = parse_widget_node(expr) {
                                render_body.push(node);
                            }
                        }
                        ast::BlockStmtOrExpr::BlockStmt(block) => {
                            let nodes = parse_render_body_stmts(&block.stmts, &family_param_name);
                            render_body = nodes;
                        }
                    }
                }
            }
            _ => {} // Skip timeline and other fields handled differently
        }
    }

    if kind.is_empty() {
        kind = "com.perry.widget".to_string();
    }

    // Fix provider func name if kind was set after provider was parsed
    if let Some(ref mut pfn) = provider_func_name {
        if pfn == "__widget_provider_widget" && kind != "com.perry.widget" {
            let safe = kind.rsplit('.').next().unwrap_or(&kind);
            *pfn = format!("__widget_provider_{}", safe);
        }
    }

    // Resolve the compile-time reload policy. The refresh interval is a single
    // compile-time constant per widget: if the provider returns several
    // distinct literal policies (e.g. a short error-retry interval and a
    // longer happy-path one), use the smallest so the most urgent request
    // wins, and tell the user.
    let reload_after_seconds: Option<u32> = if reload_policy_unparsed {
        // A policy the compiler can't read makes the whole widget's
        // interval indeterminate: honoring some *other* return path's
        // literal would silently apply that branch's interval to every
        // branch, including the unreadable one. Fall back to the platform
        // default for the widget, which is what this warning promises.
        eprintln!(
            "[perry] warning: widget '{}': `reloadPolicy` must be a literal \
`{{ after: {{ minutes: N }} }}` for the compiler to read it; a non-literal \
value was ignored and the platform default refresh interval applies \
(see docs/src/widgets/data-fetching.md)",
            kind
        );
        None
    } else {
        reload_policy_seconds.sort_unstable();
        reload_policy_seconds.dedup();
        match reload_policy_seconds.as_slice() {
            [] => None,
            [only] => Some(*only),
            many => {
                eprintln!(
                    "[perry] warning: widget '{}': provider returns {} distinct \
compile-time `reloadPolicy` values; the refresh interval is a single \
compile-time constant per widget — using the smallest ({} seconds)",
                    kind,
                    many.len(),
                    many[0]
                );
                Some(many[0])
            }
        }
    };

    Some(WidgetDecl {
        kind,
        display_name,
        description,
        supported_families,
        entry_fields,
        render_body,
        entry_param_name,
        config_params,
        provider_func_name,
        placeholder,
        family_param_name,
        app_group,
        reload_after_seconds,
    })
}

/// Extract entry fields from a typed parameter pattern (e.g., `entry: MyEntry`)
fn extract_entry_fields_from_param(pat: &ast::Pat, fields: &mut Vec<(String, WidgetFieldType)>) {
    // Try to get type annotation
    let type_ann = match pat {
        ast::Pat::Ident(ident) => ident.type_ann.as_ref(),
        _ => None,
    };
    if let Some(ann) = type_ann {
        if let ast::TsType::TsTypeLit(lit) = ann.type_ann.as_ref() {
            for member in &lit.members {
                if let ast::TsTypeElement::TsPropertySignature(prop) = member {
                    if let ast::Expr::Ident(ident) = prop.key.as_ref() {
                        let field_name = ident.sym.to_string();
                        // Skip 'date' as it's always present in TimelineEntry
                        if field_name == "date" {
                            continue;
                        }
                        let is_optional = prop.optional;
                        let field_type = if let Some(ann) = &prop.type_ann {
                            parse_widget_field_type(ann.type_ann.as_ref())
                        } else {
                            WidgetFieldType::String
                        };
                        let field_type = if is_optional {
                            WidgetFieldType::Optional(Box::new(field_type))
                        } else {
                            field_type
                        };
                        fields.push((field_name, field_type));
                    }
                }
            }
        }
    }
}

/// Issue #1179 follow-up: parse an `entryFields` value-position spec
/// into a `WidgetFieldType`. Recognized forms:
///
/// - `'string' | 'number' | 'boolean'` (plus `'string?' | 'number?' | 'boolean?'`,
///   `'string[]' | 'number[]' | 'boolean[]'`) — primitive specs;
/// - `[X]` — array literal with a single element, resolves to `Array<X>`;
/// - `{ k: <spec>, ... }` — object literal, resolves to `Object([(k, X), ...])`.
///
/// Anything else falls back to `String` for backwards compatibility with
/// existing widget configs.
fn parse_entry_field_value_spec(expr: &ast::Expr) -> WidgetFieldType {
    match expr {
        ast::Expr::Lit(ast::Lit::Str(s)) => {
            parse_entry_field_primitive_spec(s.value.as_str().unwrap_or(""))
        }
        ast::Expr::Array(arr) => {
            // Take the first non-empty element as the array's element
            // type; widgets declaring `sites: [{...}]` use a single
            // exemplar object to describe the array shape.
            let inner = arr
                .elems
                .iter()
                .flatten()
                .next()
                .map(|e| parse_entry_field_value_spec(e.expr.as_ref()))
                .unwrap_or(WidgetFieldType::String);
            WidgetFieldType::Array(Box::new(inner))
        }
        ast::Expr::Object(obj) => {
            let mut obj_fields = Vec::new();
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let field_name = prop_name_to_string(&kv.key);
                        let field_type = parse_entry_field_value_spec(kv.value.as_ref());
                        obj_fields.push((field_name, field_type));
                    }
                }
            }
            WidgetFieldType::Object(obj_fields)
        }
        _ => WidgetFieldType::String,
    }
}

/// Helper for `parse_entry_field_value_spec`: turn a primitive string
/// spec into a `WidgetFieldType`. Supports `?` (optional) and `[]`
/// (array) suffixes on the recognized base types.
fn parse_entry_field_primitive_spec(spec: &str) -> WidgetFieldType {
    let trimmed = spec.trim();
    if let Some(base) = trimmed.strip_suffix('?') {
        return WidgetFieldType::Optional(Box::new(parse_entry_field_primitive_spec(base)));
    }
    if let Some(base) = trimmed.strip_suffix("[]") {
        return WidgetFieldType::Array(Box::new(parse_entry_field_primitive_spec(base)));
    }
    match trimmed {
        "number" => WidgetFieldType::Number,
        "boolean" => WidgetFieldType::Boolean,
        "string" => WidgetFieldType::String,
        _ => WidgetFieldType::String,
    }
}

/// Recursively parse a TypeScript type annotation into a WidgetFieldType
fn parse_widget_field_type(ts_type: &ast::TsType) -> WidgetFieldType {
    match ts_type {
        ast::TsType::TsKeywordType(kw) => match kw.kind {
            ast::TsKeywordTypeKind::TsNumberKeyword => WidgetFieldType::Number,
            ast::TsKeywordTypeKind::TsBooleanKeyword => WidgetFieldType::Boolean,
            ast::TsKeywordTypeKind::TsStringKeyword => WidgetFieldType::String,
            _ => WidgetFieldType::String,
        },
        ast::TsType::TsArrayType(arr) => {
            let inner = parse_widget_field_type(arr.elem_type.as_ref());
            WidgetFieldType::Array(Box::new(inner))
        }
        ast::TsType::TsTypeLit(lit) => {
            // Nested object type: { url: string, clicks: number }
            let mut obj_fields = Vec::new();
            for member in &lit.members {
                if let ast::TsTypeElement::TsPropertySignature(prop) = member {
                    if let ast::Expr::Ident(ident) = prop.key.as_ref() {
                        let name = ident.sym.to_string();
                        let inner = if let Some(ann) = &prop.type_ann {
                            parse_widget_field_type(ann.type_ann.as_ref())
                        } else {
                            WidgetFieldType::String
                        };
                        let inner = if prop.optional {
                            WidgetFieldType::Optional(Box::new(inner))
                        } else {
                            inner
                        };
                        obj_fields.push((name, inner));
                    }
                }
            }
            WidgetFieldType::Object(obj_fields)
        }
        ast::TsType::TsUnionOrIntersectionType(ast::TsUnionOrIntersectionType::TsUnionType(
            union,
        )) => {
            // Check for T | null or T | undefined → Optional(T)
            let mut non_null_types: Vec<&ast::TsType> = Vec::new();
            let mut has_null = false;
            for member in &union.types {
                match member.as_ref() {
                    ast::TsType::TsKeywordType(kw)
                        if matches!(
                            kw.kind,
                            ast::TsKeywordTypeKind::TsNullKeyword
                                | ast::TsKeywordTypeKind::TsUndefinedKeyword
                        ) =>
                    {
                        has_null = true;
                    }
                    other => non_null_types.push(other),
                }
            }
            if has_null && non_null_types.len() == 1 {
                WidgetFieldType::Optional(Box::new(parse_widget_field_type(non_null_types[0])))
            } else if !non_null_types.is_empty() {
                parse_widget_field_type(non_null_types[0])
            } else {
                WidgetFieldType::String
            }
        }
        _ => WidgetFieldType::String,
    }
}

/// Parse a widget node from an AST expression.
/// Recognizes calls like Text("hello"), VStack({...}, [...]), Image({systemName: "star"}), etc.
fn parse_widget_node(expr: &ast::Expr) -> Option<WidgetNode> {
    match expr {
        ast::Expr::Call(call) => {
            let func_name = match &call.callee {
                ast::Callee::Expr(e) => match e.as_ref() {
                    ast::Expr::Ident(ident) => ident.sym.to_string(),
                    _ => return None,
                },
                _ => return None,
            };

            match func_name.as_str() {
                "Text" => {
                    let content = call
                        .args
                        .first()
                        .map(|arg| parse_text_content(&arg.expr))
                        .unwrap_or(WidgetTextContent::Literal(String::new()));
                    let modifiers = parse_modifiers_from_args(&call.args, 1);
                    Some(WidgetNode::Text { content, modifiers })
                }
                "VStack" | "HStack" | "ZStack" => {
                    let kind = match func_name.as_str() {
                        "VStack" => WidgetStackKind::VStack,
                        "HStack" => WidgetStackKind::HStack,
                        "ZStack" => WidgetStackKind::ZStack,
                        _ => unreachable!(),
                    };
                    parse_stack_node(kind, &call.args)
                }
                "Image" => parse_image_node(&call.args),
                "Spacer" => Some(WidgetNode::Spacer),
                "Divider" => Some(WidgetNode::Divider),
                "ForEach" => parse_foreach_node(&call.args),
                "Label" => parse_label_node(&call.args),
                "Gauge" => parse_gauge_node(&call.args),
                _ => None,
            }
        }
        ast::Expr::Cond(cond) => {
            // Ternary: condition ? then : else
            parse_conditional_node(cond)
        }
        _ => None,
    }
}

/// Parse text content from an expression
fn parse_text_content(expr: &ast::Expr) -> WidgetTextContent {
    match expr {
        ast::Expr::Lit(ast::Lit::Str(s)) => {
            WidgetTextContent::Literal(s.value.as_str().unwrap_or("").to_string())
        }
        ast::Expr::Member(member) => {
            // entry.fieldName
            if let ast::MemberProp::Ident(prop) = &member.prop {
                WidgetTextContent::Field(prop.sym.to_string())
            } else {
                WidgetTextContent::Literal(String::new())
            }
        }
        ast::Expr::Call(_) => {
            // Issue #1179 follow-up: try the whitelist of known formatters
            // (`String(x)`, `Number(x)`, `x.toFixed(n)`, `x.toString()`,
            // `Math.round/floor/ceil(x)`). Anything outside the whitelist
            // degrades to an empty literal — same behavior as before, but
            // a follow-up could surface this as a diagnostic.
            match try_parse_widget_format_expr(expr) {
                Some(fmt) => WidgetTextContent::Formatted(fmt),
                None => WidgetTextContent::Literal(String::new()),
            }
        }
        ast::Expr::Tpl(tpl) => {
            // Template literal: `Score: ${entry.score}` or
            // `Score: ${Math.round(entry.score)}`.
            let mut parts = Vec::new();
            for (i, quasi) in tpl.quasis.iter().enumerate() {
                let raw = quasi.raw.as_ref().to_string();
                if !raw.is_empty() {
                    parts.push(WidgetTemplatePart::Literal(raw));
                }
                if i < tpl.exprs.len() {
                    match tpl.exprs[i].as_ref() {
                        ast::Expr::Member(member) => {
                            if let ast::MemberProp::Ident(prop) = &member.prop {
                                parts.push(WidgetTemplatePart::Field(prop.sym.to_string()));
                            }
                        }
                        call @ ast::Expr::Call(_) => {
                            if let Some(fmt) = try_parse_widget_format_expr(call) {
                                parts.push(WidgetTemplatePart::Formatted(fmt));
                            }
                        }
                        _ => {}
                    }
                }
            }
            WidgetTextContent::Template(parts)
        }
        _ => WidgetTextContent::Literal(String::new()),
    }
}

/// Issue #1179 follow-up: recognize one of the whitelisted formatting
/// calls inside a render text position. Returns `None` for any shape
/// the codegen path can't transpile (callers fall back to an empty
/// literal in that case).
fn try_parse_widget_format_expr(expr: &ast::Expr) -> Option<WidgetFormatExpr> {
    let call = match expr {
        ast::Expr::Call(c) => c,
        _ => return None,
    };
    let callee = match &call.callee {
        ast::Callee::Expr(e) => e.as_ref(),
        _ => return None,
    };

    match callee {
        // `String(x)` / `Number(x)` — global coercion functions.
        ast::Expr::Ident(ident) => {
            let arg = call.args.first().and_then(|a| parse_format_arg(&a.expr))?;
            match ident.sym.as_ref() {
                "String" => Some(WidgetFormatExpr {
                    call: WidgetFormatCall::StringCast,
                    arg,
                }),
                "Number" => Some(WidgetFormatExpr {
                    call: WidgetFormatCall::NumberCast,
                    arg,
                }),
                _ => None,
            }
        }
        // `Math.round(x)` / `Math.floor(x)` / `Math.ceil(x)` and
        // `x.toFixed(n)` / `x.toString()` member calls.
        ast::Expr::Member(member) => {
            let prop_name = match &member.prop {
                ast::MemberProp::Ident(p) => p.sym.as_ref(),
                _ => return None,
            };
            // Math.*
            if let ast::Expr::Ident(obj) = member.obj.as_ref() {
                if obj.sym.as_ref() == "Math" {
                    let arg = call.args.first().and_then(|a| parse_format_arg(&a.expr))?;
                    let call_kind = match prop_name {
                        "round" => WidgetFormatCall::Round,
                        "floor" => WidgetFormatCall::Floor,
                        "ceil" => WidgetFormatCall::Ceil,
                        _ => return None,
                    };
                    return Some(WidgetFormatExpr {
                        call: call_kind,
                        arg,
                    });
                }
            }
            // x.toFixed(n) / x.toString() — `x` must be a member access
            // (entry field) for us to handle it.
            let arg = match member.obj.as_ref() {
                ast::Expr::Member(inner) => match &inner.prop {
                    ast::MemberProp::Ident(p) => WidgetFormatArg::Field(p.sym.to_string()),
                    _ => return None,
                },
                _ => return None,
            };
            match prop_name {
                "toString" if call.args.is_empty() => Some(WidgetFormatExpr {
                    call: WidgetFormatCall::ToString,
                    arg,
                }),
                "toFixed" => {
                    let digits = call
                        .args
                        .first()
                        .and_then(|a| match a.expr.as_ref() {
                            ast::Expr::Lit(ast::Lit::Num(n)) => Some(n.value),
                            _ => None,
                        })
                        .filter(|n| n.is_finite() && *n >= 0.0)
                        .map(|n| n as u32)
                        .unwrap_or(0);
                    Some(WidgetFormatExpr {
                        call: WidgetFormatCall::ToFixed { digits },
                        arg,
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Parse an argument expression into a `WidgetFormatArg`. Recognized
/// shapes are `entry.<field>` (member access), numeric literal, and
/// string literal. Anything else returns `None` and bubbles up to a
/// non-recognized format call.
fn parse_format_arg(expr: &ast::Expr) -> Option<WidgetFormatArg> {
    match expr {
        ast::Expr::Member(member) => match &member.prop {
            ast::MemberProp::Ident(p) => Some(WidgetFormatArg::Field(p.sym.to_string())),
            _ => None,
        },
        ast::Expr::Lit(ast::Lit::Num(n)) => Some(WidgetFormatArg::Number(n.value)),
        ast::Expr::Lit(ast::Lit::Str(s)) => Some(WidgetFormatArg::String(
            s.value.as_str().unwrap_or("").to_string(),
        )),
        _ => None,
    }
}

/// Parse a stack node (VStack, HStack, ZStack) from call arguments.
/// Supports two patterns:
///   VStack([child1, child2])
///   VStack({ spacing: 8 }, [child1, child2])
fn parse_stack_node(kind: WidgetStackKind, args: &[ast::ExprOrSpread]) -> Option<WidgetNode> {
    let mut spacing = None;
    let mut children = Vec::new();
    // #854: `modifiers` was initialized to `Vec::new()` but always
    // overwritten unconditionally by `parse_modifiers_from_args` below.
    // Declared without an initial value.
    let mut children_arg_idx = 0;

    // Check if first arg is config object
    if let Some(first) = args.first() {
        match first.expr.as_ref() {
            ast::Expr::Object(obj) => {
                // First arg is config: { spacing: 8 }
                for prop in &obj.props {
                    if let ast::PropOrSpread::Prop(p) = prop {
                        if let ast::Prop::KeyValue(kv) = p.as_ref() {
                            let key = prop_name_to_string(&kv.key);
                            if key == "spacing" {
                                if let ast::Expr::Lit(ast::Lit::Num(n)) = kv.value.as_ref() {
                                    spacing = Some(n.value);
                                }
                            }
                        }
                    }
                }
                children_arg_idx = 1;
            }
            ast::Expr::Array(_) => {
                // First arg is children array directly
                children_arg_idx = 0;
            }
            _ => {}
        }
    }

    // Parse children array
    if let Some(arg) = args.get(children_arg_idx) {
        if let ast::Expr::Array(arr) = arg.expr.as_ref() {
            for ast::ExprOrSpread { expr, .. } in arr.elems.iter().flatten() {
                if let Some(node) = parse_widget_node(expr) {
                    children.push(node);
                }
            }
        }
    }

    // Parse modifiers from remaining args
    let modifier_start = children_arg_idx + 1;
    let modifiers = parse_modifiers_from_args(args, modifier_start);

    Some(WidgetNode::Stack {
        kind,
        spacing,
        children,
        modifiers,
    })
}

/// Parse an Image node from call arguments.
/// Image({ systemName: "star.fill" })
fn parse_image_node(args: &[ast::ExprOrSpread]) -> Option<WidgetNode> {
    let first = args.first()?;
    let system_name = match first.expr.as_ref() {
        ast::Expr::Object(obj) => {
            let mut name = String::new();
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let key = prop_name_to_string(&kv.key);
                        if key == "systemName" {
                            if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                                name = s.value.as_str().unwrap_or("").to_string();
                            }
                        }
                    }
                }
            }
            name
        }
        ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or("").to_string(),
        _ => return None,
    };

    let modifiers = parse_modifiers_from_args(args, 1);
    Some(WidgetNode::Image {
        system_name,
        modifiers,
    })
}

/// Parse a conditional node from a ternary expression
fn parse_conditional_node(cond: &ast::CondExpr) -> Option<WidgetNode> {
    // Parse condition: entry.field > value, entry.field == value, etc.
    let (field, op, value) = parse_condition(&cond.test)?;
    let then_node = parse_widget_node(&cond.cons)?;
    let else_node = parse_widget_node(&cond.alt);

    Some(WidgetNode::Conditional {
        field,
        op,
        value,
        then_node: Box::new(then_node),
        else_node: else_node.map(Box::new),
    })
}

/// Parse a binary condition expression
fn parse_condition(expr: &ast::Expr) -> Option<(String, WidgetConditionOp, WidgetTextContent)> {
    match expr {
        ast::Expr::Bin(bin) => {
            let field = match bin.left.as_ref() {
                ast::Expr::Member(member) => {
                    if let ast::MemberProp::Ident(prop) = &member.prop {
                        prop.sym.to_string()
                    } else {
                        return None;
                    }
                }
                _ => return None,
            };
            let op = match bin.op {
                ast::BinaryOp::Gt => WidgetConditionOp::GreaterThan,
                ast::BinaryOp::Lt => WidgetConditionOp::LessThan,
                ast::BinaryOp::EqEq | ast::BinaryOp::EqEqEq => WidgetConditionOp::Equals,
                ast::BinaryOp::NotEq | ast::BinaryOp::NotEqEq => WidgetConditionOp::NotEquals,
                _ => return None,
            };
            let value = parse_text_content(&bin.right);
            Some((field, op, value))
        }
        ast::Expr::Member(member) => {
            // Truthy check: entry.isActive
            if let ast::MemberProp::Ident(prop) = &member.prop {
                Some((
                    prop.sym.to_string(),
                    WidgetConditionOp::Truthy,
                    WidgetTextContent::Literal(String::new()),
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Parse modifiers from a chained method call or from arguments.
/// In the TypeScript API, modifiers are passed as the last argument (object):
///   Text("hello", { font: "title", fontWeight: "bold", foregroundColor: "blue" })
fn parse_modifiers_from_args(args: &[ast::ExprOrSpread], start_idx: usize) -> Vec<WidgetModifier> {
    let mut modifiers = Vec::new();
    if let Some(arg) = args.get(start_idx) {
        if let ast::Expr::Object(obj) = arg.expr.as_ref() {
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let key = prop_name_to_string(&kv.key);
                        if let Some(m) = parse_single_modifier(&key, &kv.value) {
                            modifiers.push(m);
                        }
                    }
                }
            }
        }
    }
    modifiers
}

/// Returns true if `name` is a known widget modifier key (used to detect
/// unsupported method-chain modifier calls, e.g. `Text("hi").font("title")`).
pub(super) fn is_widget_modifier_name(name: &str) -> bool {
    matches!(
        name,
        "font"
            | "fontWeight"
            | "weight"
            | "foregroundColor"
            | "color"
            | "foreground"
            | "padding"
            | "cornerRadius"
            | "background"
            | "backgroundColor"
            | "opacity"
            | "lineLimit"
            | "frame"
            | "minimumScaleFactor"
            | "containerBackground"
            | "maxWidth"
            | "url"
            | "bold"
            | "italic"
            | "underline"
            | "fontSize"
            | "strikethrough"
            | "multilineTextAlignment"
            | "lineSpacing"
    )
}

/// Parse a single modifier from key/value
fn parse_single_modifier(key: &str, value: &ast::Expr) -> Option<WidgetModifier> {
    match key {
        "font" => match value {
            ast::Expr::Lit(ast::Lit::Str(s)) => {
                let font = match s.value.as_str().unwrap_or("") {
                    "headline" => WidgetFont::Headline,
                    "title" => WidgetFont::Title,
                    "title2" => WidgetFont::Title2,
                    "title3" => WidgetFont::Title3,
                    "body" => WidgetFont::Body,
                    "caption" => WidgetFont::Caption,
                    "caption2" => WidgetFont::Caption2,
                    "footnote" => WidgetFont::Footnote,
                    "subheadline" => WidgetFont::Subheadline,
                    "largeTitle" => WidgetFont::LargeTitle,
                    name => WidgetFont::Named(name.to_string()),
                };
                Some(WidgetModifier::Font(font))
            }
            ast::Expr::Lit(ast::Lit::Num(n)) => {
                Some(WidgetModifier::Font(WidgetFont::System(n.value)))
            }
            _ => None,
        },
        "fontWeight" | "weight" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::FontWeight(
                    s.value.as_str().unwrap_or("").to_string(),
                ))
            } else {
                None
            }
        }
        "foregroundColor" | "color" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::ForegroundColor(
                    s.value.as_str().unwrap_or("").to_string(),
                ))
            } else {
                None
            }
        }
        "padding" => {
            if let ast::Expr::Lit(ast::Lit::Num(n)) = value {
                Some(WidgetModifier::Padding(n.value))
            } else {
                None
            }
        }
        "cornerRadius" => {
            if let ast::Expr::Lit(ast::Lit::Num(n)) = value {
                Some(WidgetModifier::CornerRadius(n.value))
            } else {
                None
            }
        }
        "background" | "backgroundColor" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::Background(
                    s.value.as_str().unwrap_or("").to_string(),
                ))
            } else {
                None
            }
        }
        "opacity" => {
            if let ast::Expr::Lit(ast::Lit::Num(n)) = value {
                Some(WidgetModifier::Opacity(n.value))
            } else {
                None
            }
        }
        "lineLimit" => {
            if let ast::Expr::Lit(ast::Lit::Num(n)) = value {
                Some(WidgetModifier::LineLimit(n.value as u32))
            } else {
                None
            }
        }
        "frame" => {
            if let ast::Expr::Object(obj) = value {
                let mut width = None;
                let mut height = None;
                for prop in &obj.props {
                    if let ast::PropOrSpread::Prop(p) = prop {
                        if let ast::Prop::KeyValue(kv) = p.as_ref() {
                            let k = prop_name_to_string(&kv.key);
                            if let ast::Expr::Lit(ast::Lit::Num(n)) = kv.value.as_ref() {
                                match k.as_str() {
                                    "width" => width = Some(n.value),
                                    "height" => height = Some(n.value),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                Some(WidgetModifier::Frame { width, height })
            } else {
                None
            }
        }
        "minimumScaleFactor" => {
            if let ast::Expr::Lit(ast::Lit::Num(n)) = value {
                Some(WidgetModifier::MinimumScaleFactor(n.value))
            } else {
                None
            }
        }
        "containerBackground" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::ContainerBackground(
                    s.value.as_str().unwrap_or("").to_string(),
                ))
            } else {
                None
            }
        }
        "maxWidth" => {
            // maxWidth: true or maxWidth: "infinity"
            Some(WidgetModifier::FrameMaxWidth)
        }
        "url" => {
            if let ast::Expr::Lit(ast::Lit::Str(s)) = value {
                Some(WidgetModifier::WidgetURL(
                    s.value.as_str().unwrap_or("").to_string(),
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Parse a ForEach node: ForEach(entry.items, (item) => HStack([...]))
fn parse_foreach_node(args: &[ast::ExprOrSpread]) -> Option<WidgetNode> {
    // First arg: entry.items (member expression)
    let collection_field = match args.first()?.expr.as_ref() {
        ast::Expr::Member(member) => {
            if let ast::MemberProp::Ident(prop) = &member.prop {
                prop.sym.to_string()
            } else {
                return None;
            }
        }
        _ => return None,
    };

    // Second arg: arrow function (item) => ...
    let arrow = match args.get(1)?.expr.as_ref() {
        ast::Expr::Arrow(arrow) => arrow,
        _ => return None,
    };

    let item_param = if let Some(param) = arrow.params.first() {
        if let ast::Pat::Ident(ident) = param {
            ident.id.sym.to_string()
        } else {
            "item".to_string()
        }
    } else {
        "item".to_string()
    };

    let body = match arrow.body.as_ref() {
        ast::BlockStmtOrExpr::Expr(expr) => parse_widget_node(expr)?,
        ast::BlockStmtOrExpr::BlockStmt(block) => {
            for stmt in &block.stmts {
                if let ast::Stmt::Return(ret) = stmt {
                    if let Some(arg) = &ret.arg {
                        if let Some(node) = parse_widget_node(arg) {
                            return Some(WidgetNode::ForEach {
                                collection_field,
                                item_param,
                                body: Box::new(node),
                            });
                        }
                    }
                }
            }
            return None;
        }
    };

    Some(WidgetNode::ForEach {
        collection_field,
        item_param,
        body: Box::new(body),
    })
}

/// Parse a Label node: Label("text", { systemImage: "star.fill" })
fn parse_label_node(args: &[ast::ExprOrSpread]) -> Option<WidgetNode> {
    let text = args
        .first()
        .map(|arg| parse_text_content(&arg.expr))
        .unwrap_or(WidgetTextContent::Literal(String::new()));

    let mut system_image = String::new();
    let mut modifiers = Vec::new();

    // Second arg: { systemImage: "star.fill", font: "caption" }
    if let Some(arg) = args.get(1) {
        if let ast::Expr::Object(obj) = arg.expr.as_ref() {
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let key = prop_name_to_string(&kv.key);
                        if key == "systemImage" {
                            if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                                system_image = s.value.as_str().unwrap_or("").to_string();
                            }
                        } else if let Some(m) = parse_single_modifier(&key, &kv.value) {
                            modifiers.push(m);
                        }
                    }
                }
            }
        }
    }

    Some(WidgetNode::Label {
        text,
        system_image,
        modifiers,
    })
}

/// Parse a Gauge node: Gauge(value, { label: "Clicks", style: "circular" })
fn parse_gauge_node(args: &[ast::ExprOrSpread]) -> Option<WidgetNode> {
    // First arg: value expression (entry.field / entry.field, or numeric expression)
    let value_expr = match args.first()?.expr.as_ref() {
        ast::Expr::Member(member) => {
            if let ast::MemberProp::Ident(prop) = &member.prop {
                prop.sym.to_string()
            } else {
                return None;
            }
        }
        ast::Expr::Bin(bin) => {
            // entry.totalClicks / entry.clicksGoal
            let left = match bin.left.as_ref() {
                ast::Expr::Member(m) => {
                    if let ast::MemberProp::Ident(p) = &m.prop {
                        p.sym.to_string()
                    } else {
                        return None;
                    }
                }
                _ => return None,
            };
            let right = match bin.right.as_ref() {
                ast::Expr::Member(m) => {
                    if let ast::MemberProp::Ident(p) = &m.prop {
                        p.sym.to_string()
                    } else {
                        return None;
                    }
                }
                ast::Expr::Lit(ast::Lit::Num(n)) => format!("{}", n.value),
                _ => return None,
            };
            let op = match bin.op {
                ast::BinaryOp::Div => "/",
                ast::BinaryOp::Mul => "*",
                ast::BinaryOp::Sub => "-",
                ast::BinaryOp::Add => "+",
                _ => return None,
            };
            format!("{} {} {}", left, op, right)
        }
        _ => return None,
    };

    let mut label = String::new();
    let mut style = GaugeStyle::Circular;
    let mut modifiers = Vec::new();

    // Second arg: config object
    if let Some(arg) = args.get(1) {
        if let ast::Expr::Object(obj) = arg.expr.as_ref() {
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let key = prop_name_to_string(&kv.key);
                        match key.as_str() {
                            "label" => {
                                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                                    label = s.value.as_str().unwrap_or("").to_string();
                                }
                            }
                            "style" => {
                                if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                                    style = match s.value.as_str().unwrap_or("") {
                                        "linear" | "linearCapacity" => GaugeStyle::LinearCapacity,
                                        _ => GaugeStyle::Circular,
                                    };
                                }
                            }
                            _ => {
                                if let Some(m) = parse_single_modifier(&key, &kv.value) {
                                    modifiers.push(m);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Some(WidgetNode::Gauge {
        value_expr,
        label,
        style,
        modifiers,
    })
}

/// Parse render body statements, detecting family-switch patterns (if/else on family param)
fn parse_render_body_stmts(stmts: &[ast::Stmt], family_param: &Option<String>) -> Vec<WidgetNode> {
    let mut nodes = Vec::new();

    // Check for if (family === "systemSmall") { ... } else if ... pattern
    if let Some(family_name) = family_param {
        if let Some(family_switch) = try_parse_family_switch(stmts, family_name) {
            nodes.push(family_switch);
            return nodes;
        }
    }

    // Fall back to regular return-based parsing
    for stmt in stmts {
        if let ast::Stmt::Return(ret) = stmt {
            if let Some(arg) = &ret.arg {
                if let Some(node) = parse_widget_node(arg) {
                    nodes.push(node);
                }
            }
        }
    }
    nodes
}

/// Try to parse a series of if (family === "X") { return ... } statements into a FamilySwitch
fn try_parse_family_switch(stmts: &[ast::Stmt], family_name: &str) -> Option<WidgetNode> {
    let mut cases: Vec<(String, WidgetNode)> = Vec::new();
    let mut default_node: Option<Box<WidgetNode>> = None;

    for stmt in stmts {
        match stmt {
            ast::Stmt::If(if_stmt) => {
                // Check: if (family === "systemSmall") { return VStack([...]) }
                if let Some((family_value, node)) =
                    try_parse_family_case(&if_stmt.test, &if_stmt.cons, family_name)
                {
                    cases.push((family_value, node));
                }
                // Check else branch for more cases or default
                if let Some(alt) = &if_stmt.alt {
                    match alt.as_ref() {
                        ast::Stmt::Block(block) => {
                            // else { return ... } — this is the default
                            for s in &block.stmts {
                                if let ast::Stmt::Return(ret) = s {
                                    if let Some(arg) = &ret.arg {
                                        if let Some(node) = parse_widget_node(arg) {
                                            default_node = Some(Box::new(node));
                                        }
                                    }
                                }
                            }
                        }
                        ast::Stmt::If(nested_if) => {
                            // else if — extract more cases
                            if let Some((family_value, node)) =
                                try_parse_family_case(&nested_if.test, &nested_if.cons, family_name)
                            {
                                cases.push((family_value, node));
                            }
                        }
                        ast::Stmt::Return(ret) => {
                            if let Some(arg) = &ret.arg {
                                if let Some(node) = parse_widget_node(arg) {
                                    default_node = Some(Box::new(node));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            ast::Stmt::Return(ret) => {
                // Trailing return is the default case
                if let Some(arg) = &ret.arg {
                    if let Some(node) = parse_widget_node(arg) {
                        if cases.is_empty() {
                            // No family switch, just a regular return
                            return None;
                        }
                        default_node = Some(Box::new(node));
                    }
                }
            }
            _ => {}
        }
    }

    if cases.is_empty() {
        return None;
    }

    Some(WidgetNode::FamilySwitch {
        cases,
        default: default_node,
    })
}

/// Try to parse a single if (family === "value") { return node } case
fn try_parse_family_case(
    test: &ast::Expr,
    cons: &ast::Stmt,
    family_name: &str,
) -> Option<(String, WidgetNode)> {
    // Check: family === "systemSmall"
    let family_value = match test {
        ast::Expr::Bin(bin) if matches!(bin.op, ast::BinaryOp::EqEqEq | ast::BinaryOp::EqEq) => {
            let is_family_left = match bin.left.as_ref() {
                ast::Expr::Ident(ident) => ident.sym.as_ref() == family_name,
                _ => false,
            };
            if !is_family_left {
                return None;
            }
            match bin.right.as_ref() {
                ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().unwrap_or("").to_string(),
                _ => return None,
            }
        }
        _ => return None,
    };

    // Extract return value from consequent block
    let node = match cons {
        ast::Stmt::Block(block) => {
            let mut result = None;
            for s in &block.stmts {
                if let ast::Stmt::Return(ret) = s {
                    if let Some(arg) = &ret.arg {
                        result = parse_widget_node(arg);
                    }
                }
            }
            result?
        }
        ast::Stmt::Return(ret) => {
            if let Some(arg) = &ret.arg {
                parse_widget_node(arg)?
            } else {
                return None;
            }
        }
        _ => return None,
    };

    Some((family_value, node))
}

/// Parse a WidgetConfigParam from a config field value
fn parse_widget_config_param(name: &str, value: &ast::Expr) -> Option<WidgetConfigParam> {
    if let ast::Expr::Object(obj) = value {
        let mut param_type_str = String::new();
        let mut title = name.to_string();
        let mut values: Vec<String> = Vec::new();
        let mut default_str = String::new();
        let mut default_bool = false;

        for prop in &obj.props {
            if let ast::PropOrSpread::Prop(p) = prop {
                if let ast::Prop::KeyValue(kv) = p.as_ref() {
                    let key = prop_name_to_string(&kv.key);
                    match key.as_str() {
                        "type" => {
                            if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                                param_type_str = s.value.as_str().unwrap_or("").to_string();
                            }
                        }
                        "title" => {
                            if let ast::Expr::Lit(ast::Lit::Str(s)) = kv.value.as_ref() {
                                title = s.value.as_str().unwrap_or("").to_string();
                            }
                        }
                        "default" => match kv.value.as_ref() {
                            ast::Expr::Lit(ast::Lit::Str(s)) => {
                                default_str = s.value.as_str().unwrap_or("").to_string();
                            }
                            ast::Expr::Lit(ast::Lit::Bool(b)) => {
                                default_bool = b.value;
                            }
                            _ => {}
                        },
                        "values" => {
                            if let ast::Expr::Array(arr) = kv.value.as_ref() {
                                for ast::ExprOrSpread { expr, .. } in arr.elems.iter().flatten() {
                                    if let ast::Expr::Lit(ast::Lit::Str(s)) = expr.as_ref() {
                                        values.push(s.value.as_str().unwrap_or("").to_string());
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        let param_type = match param_type_str.as_str() {
            "enum" => WidgetConfigParamType::Enum {
                values,
                default: if default_str.is_empty() {
                    "".to_string()
                } else {
                    default_str
                },
            },
            "bool" | "boolean" => WidgetConfigParamType::Bool {
                default: default_bool,
            },
            "string" => WidgetConfigParamType::String {
                default: default_str,
            },
            _ => WidgetConfigParamType::String {
                default: default_str,
            },
        };

        Some(WidgetConfigParam {
            name: name.to_string(),
            title,
            param_type,
        })
    } else {
        None
    }
}

/// Strip semantically transparent wrappers (parens, TS casts) so return-value
/// scanning sees the underlying object literal in shapes like
/// `return ({ ... } as ProviderResult)`.
fn strip_expr_wrappers(expr: &ast::Expr) -> &ast::Expr {
    match expr {
        ast::Expr::Paren(p) => strip_expr_wrappers(&p.expr),
        ast::Expr::TsAs(a) => strip_expr_wrappers(&a.expr),
        ast::Expr::TsNonNull(n) => strip_expr_wrappers(&n.expr),
        ast::Expr::TsTypeAssertion(a) => strip_expr_wrappers(&a.expr),
        ast::Expr::TsConstAssertion(a) => strip_expr_wrappers(&a.expr),
        ast::Expr::TsSatisfies(s) => strip_expr_wrappers(&s.expr),
        _ => expr,
    }
}

/// Parse a `reloadPolicy` value literal into whole seconds. The single
/// documented form is `{ after: { minutes: N } }` with a numeric literal `N`
/// (`N` may be fractional; the result is rounded to the nearest second).
/// Returns `None` for anything else — non-literal shapes can't be read at
/// compile time.
fn parse_reload_policy_seconds(expr: &ast::Expr) -> Option<u32> {
    let obj = match strip_expr_wrappers(expr) {
        ast::Expr::Object(obj) => obj,
        _ => return None,
    };
    for prop in &obj.props {
        let ast::PropOrSpread::Prop(p) = prop else {
            continue;
        };
        let ast::Prop::KeyValue(kv) = p.as_ref() else {
            continue;
        };
        if prop_name_to_string(&kv.key) != "after" {
            continue;
        }
        let after_obj = match strip_expr_wrappers(kv.value.as_ref()) {
            ast::Expr::Object(o) => o,
            _ => return None,
        };
        for after_prop in &after_obj.props {
            let ast::PropOrSpread::Prop(ap) = after_prop else {
                continue;
            };
            let ast::Prop::KeyValue(akv) = ap.as_ref() else {
                continue;
            };
            if prop_name_to_string(&akv.key) != "minutes" {
                continue;
            }
            let ast::Expr::Lit(ast::Lit::Num(n)) = strip_expr_wrappers(akv.value.as_ref()) else {
                return None;
            };
            let minutes = n.value;
            if !minutes.is_finite() || minutes <= 0.0 {
                return None;
            }
            let seconds = (minutes * 60.0).round();
            if seconds < 1.0 {
                return Some(1);
            }
            if seconds >= u32::MAX as f64 {
                return Some(u32::MAX);
            }
            return Some(seconds as u32);
        }
        return None;
    }
    None
}

/// Inspect one provider return-value expression for a `reloadPolicy`
/// property. Literal policies are appended to `found` (in seconds); a
/// `reloadPolicy` that exists but isn't a readable literal sets `unparsed`.
fn scan_provider_return_expr_for_reload_policy(
    expr: &ast::Expr,
    found: &mut Vec<u32>,
    unparsed: &mut bool,
) {
    let obj = match strip_expr_wrappers(expr) {
        ast::Expr::Object(obj) => obj,
        _ => return,
    };
    for prop in &obj.props {
        let ast::PropOrSpread::Prop(p) = prop else {
            continue;
        };
        let ast::Prop::KeyValue(kv) = p.as_ref() else {
            continue;
        };
        if prop_name_to_string(&kv.key) != "reloadPolicy" {
            continue;
        }
        match parse_reload_policy_seconds(kv.value.as_ref()) {
            Some(seconds) => found.push(seconds),
            None => *unparsed = true,
        }
    }
}

/// Walk a provider function body and collect every compile-time
/// `reloadPolicy` from its `return` statements, recursing through the
/// statement shapes a provider realistically uses (blocks, if/else, loops,
/// try/catch, switch, labels).
fn scan_provider_stmts_for_reload_policy(
    stmts: &[ast::Stmt],
    found: &mut Vec<u32>,
    unparsed: &mut bool,
) {
    for stmt in stmts {
        scan_provider_stmt_for_reload_policy(stmt, found, unparsed);
    }
}

fn scan_provider_stmt_for_reload_policy(
    stmt: &ast::Stmt,
    found: &mut Vec<u32>,
    unparsed: &mut bool,
) {
    match stmt {
        ast::Stmt::Return(ret) => {
            if let Some(arg) = &ret.arg {
                scan_provider_return_expr_for_reload_policy(arg, found, unparsed);
            }
        }
        ast::Stmt::Block(block) => {
            scan_provider_stmts_for_reload_policy(&block.stmts, found, unparsed);
        }
        ast::Stmt::If(if_stmt) => {
            scan_provider_stmt_for_reload_policy(&if_stmt.cons, found, unparsed);
            if let Some(alt) = &if_stmt.alt {
                scan_provider_stmt_for_reload_policy(alt, found, unparsed);
            }
        }
        ast::Stmt::While(w) => scan_provider_stmt_for_reload_policy(&w.body, found, unparsed),
        ast::Stmt::DoWhile(d) => scan_provider_stmt_for_reload_policy(&d.body, found, unparsed),
        ast::Stmt::For(f) => scan_provider_stmt_for_reload_policy(&f.body, found, unparsed),
        ast::Stmt::ForIn(f) => scan_provider_stmt_for_reload_policy(&f.body, found, unparsed),
        ast::Stmt::ForOf(f) => scan_provider_stmt_for_reload_policy(&f.body, found, unparsed),
        ast::Stmt::Try(t) => {
            scan_provider_stmts_for_reload_policy(&t.block.stmts, found, unparsed);
            if let Some(handler) = &t.handler {
                scan_provider_stmts_for_reload_policy(&handler.body.stmts, found, unparsed);
            }
            if let Some(finalizer) = &t.finalizer {
                scan_provider_stmts_for_reload_policy(&finalizer.stmts, found, unparsed);
            }
        }
        ast::Stmt::Switch(s) => {
            for case in &s.cases {
                scan_provider_stmts_for_reload_policy(&case.cons, found, unparsed);
            }
        }
        ast::Stmt::Labeled(l) => scan_provider_stmt_for_reload_policy(&l.body, found, unparsed),
        _ => {}
    }
}

/// Parse a placeholder value from an expression
fn parse_placeholder_value(expr: &ast::Expr) -> WidgetPlaceholderValue {
    match expr {
        ast::Expr::Lit(ast::Lit::Str(s)) => {
            WidgetPlaceholderValue::String(s.value.as_str().unwrap_or("").to_string())
        }
        ast::Expr::Lit(ast::Lit::Num(n)) => WidgetPlaceholderValue::Number(n.value),
        ast::Expr::Lit(ast::Lit::Bool(b)) => WidgetPlaceholderValue::Bool(b.value),
        ast::Expr::Lit(ast::Lit::Null(_)) => WidgetPlaceholderValue::Null,
        ast::Expr::Array(arr) => {
            let items: Vec<WidgetPlaceholderValue> = arr
                .elems
                .iter()
                .filter_map(|e| e.as_ref())
                .map(|e| parse_placeholder_value(&e.expr))
                .collect();
            WidgetPlaceholderValue::Array(items)
        }
        ast::Expr::Object(obj) => {
            let mut fields = Vec::new();
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let name = prop_name_to_string(&kv.key);
                        let val = parse_placeholder_value(&kv.value);
                        fields.push((name, val));
                    }
                }
            }
            WidgetPlaceholderValue::Object(fields)
        }
        _ => WidgetPlaceholderValue::Null,
    }
}

#[cfg(test)]
mod tests;
