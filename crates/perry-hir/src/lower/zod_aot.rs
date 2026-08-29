//! Prototype Zod schema IR and direct HIR lowering.
//!
//! This deliberately does not compile Zod's generated JavaScript. It reads a
//! closed, static schema expression into [`ZodSchemaIr`] and emits only a
//! native specialized parser. Zod's `compileFromParser` integration point owns
//! the schema clone, parse-context bypasses, public methods, and runtime
//! fallback. Unsupported/error paths stay entirely in Zod.

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::{CompareOp, Expr, LogicalOp, Param, Stmt, UnaryOp};
use crate::types::Type;

use super::{lower_expr, LoweringContext};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ZodSchemaIr {
    Object(Vec<ZodFieldIr>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ZodFieldIr {
    pub(crate) key: String,
    pub(crate) value: ZodValueIr,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ZodValueIr {
    String,
    Number {
        integer: bool,
        minimum: Option<f64>,
        maximum: Option<f64>,
    },
    Boolean,
}

fn peel_expr(mut expr: &ast::Expr) -> &ast::Expr {
    loop {
        expr = match expr {
            ast::Expr::Paren(x) => &x.expr,
            ast::Expr::TsAs(x) => &x.expr,
            ast::Expr::TsNonNull(x) => &x.expr,
            ast::Expr::TsSatisfies(x) => &x.expr,
            ast::Expr::TsTypeAssertion(x) => &x.expr,
            ast::Expr::TsConstAssertion(x) => &x.expr,
            _ => return expr,
        };
    }
}

fn member_name(member: &ast::MemberExpr) -> Option<&str> {
    match &member.prop {
        ast::MemberProp::Ident(id) => Some(id.sym.as_ref()),
        ast::MemberProp::Computed(c) => match peel_expr(&c.expr) {
            ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str(),
            _ => None,
        },
        ast::MemberProp::PrivateName(_) => None,
    }
}

fn call_member(call: &ast::CallExpr) -> Option<(&ast::Expr, &str)> {
    let ast::Callee::Expr(callee) = &call.callee else {
        return None;
    };
    let ast::Expr::Member(member) = peel_expr(callee) else {
        return None;
    };
    Some((peel_expr(&member.obj), member_name(member)?))
}

fn is_zod_namespace(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    let ast::Expr::Ident(id) = peel_expr(expr) else {
        return false;
    };
    // Namespace imports are not local-slot bindings in Perry's HIR. Any local
    // with the same spelling therefore proves that this occurrence resolves
    // to a shadowing user binding instead of the imported Zod namespace.
    if ctx.lookup_local(id.sym.as_ref()).is_some() {
        return false;
    }
    matches!(
        ctx.namespace_import_sources
            .get(id.sym.as_ref())
            .map(String::as_str),
        Some("zod" | "zod/v4")
    )
}

fn number_literal(expr: &ast::Expr) -> Option<f64> {
    match peel_expr(expr) {
        ast::Expr::Lit(ast::Lit::Num(n)) if n.value.is_finite() => Some(n.value),
        ast::Expr::Unary(unary) if unary.op == ast::UnaryOp::Minus => {
            let ast::Expr::Lit(ast::Lit::Num(n)) = peel_expr(&unary.arg) else {
                return None;
            };
            let value = -n.value;
            value.is_finite().then_some(value)
        }
        _ => None,
    }
}

fn extract_value_ir(ctx: &LoweringContext, expr: &ast::Expr) -> Option<ZodValueIr> {
    let ast::Expr::Call(call) = peel_expr(expr) else {
        return None;
    };
    if call.args.iter().any(|arg| arg.spread.is_some()) {
        return None;
    }
    let (receiver, method) = call_member(call)?;

    if is_zod_namespace(ctx, receiver) {
        if !call.args.is_empty() {
            return None;
        }
        return match method {
            "string" => Some(ZodValueIr::String),
            "number" => Some(ZodValueIr::Number {
                integer: false,
                minimum: None,
                maximum: None,
            }),
            "boolean" => Some(ZodValueIr::Boolean),
            _ => None,
        };
    }

    let mut inner = extract_value_ir(ctx, receiver)?;
    match (&mut inner, method) {
        (
            ZodValueIr::Number {
                integer,
                minimum: _,
                maximum: _,
            },
            "int",
        ) if call.args.is_empty() => {
            *integer = true;
            Some(inner)
        }
        (ZodValueIr::Number { minimum, .. }, "min") if call.args.len() == 1 => {
            *minimum = Some(number_literal(&call.args[0].expr)?);
            Some(inner)
        }
        (ZodValueIr::Number { maximum, .. }, "max") if call.args.len() == 1 => {
            *maximum = Some(number_literal(&call.args[0].expr)?);
            Some(inner)
        }
        _ => None,
    }
}

fn object_key(name: &ast::PropName) -> Option<String> {
    match name {
        ast::PropName::Ident(id) => Some(id.sym.to_string()),
        ast::PropName::Str(s) => Some(s.value.to_string_lossy().to_string()),
        ast::PropName::Num(n) => Some(n.value.to_string()),
        _ => None,
    }
}

pub(crate) fn extract_schema_ir(ctx: &LoweringContext, expr: &ast::Expr) -> Option<ZodSchemaIr> {
    let ast::Expr::Call(call) = peel_expr(expr) else {
        return None;
    };
    if call.args.len() != 1 || call.args[0].spread.is_some() {
        return None;
    }
    let (receiver, method) = call_member(call)?;
    if method != "object" || !is_zod_namespace(ctx, receiver) {
        return None;
    }
    let ast::Expr::Object(shape) = peel_expr(&call.args[0].expr) else {
        return None;
    };

    let mut fields = Vec::with_capacity(shape.props.len());
    let mut keys = std::collections::HashSet::with_capacity(shape.props.len());
    for property in &shape.props {
        let ast::PropOrSpread::Prop(property) = property else {
            return None;
        };
        let ast::Prop::KeyValue(kv) = property.as_ref() else {
            return None;
        };
        let key = object_key(&kv.key)?;
        if key == "__proto__" || !keys.insert(key.clone()) {
            return None;
        }
        fields.push(ZodFieldIr {
            key,
            value: extract_value_ir(ctx, &kv.value)?,
        });
    }
    Some(ZodSchemaIr::Object(fields))
}

fn property(object: Expr, name: impl Into<String>) -> Expr {
    Expr::PropertyGet {
        object: Box::new(object),
        property: name.into(),
        byte_offset: 0,
    }
}

fn call(callee: Expr, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(callee),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

fn compare(op: CompareOp, left: Expr, right: Expr) -> Expr {
    Expr::Compare {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn logical(op: LogicalOp, left: Expr, right: Expr) -> Expr {
    Expr::Logical {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn not(expr: Expr) -> Expr {
    Expr::Unary {
        op: UnaryOp::Not,
        operand: Box::new(expr),
    }
}

fn return_invalid_if(condition: Expr, invalid_id: u32) -> Stmt {
    Stmt::If {
        condition,
        then_branch: vec![Stmt::Return(Some(Expr::LocalGet(invalid_id)))],
        else_branch: None,
    }
}

fn param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn closure(
    func_id: u32,
    params: Vec<Param>,
    body: Vec<Stmt>,
    captures: Vec<u32>,
    strict: bool,
) -> Expr {
    Expr::Closure {
        func_id,
        params,
        return_type: Type::Any,
        body,
        captures,
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: strict,
    }
}

fn lower_native_parser(ctx: &mut LoweringContext, ir: &ZodSchemaIr, invalid_id: u32) -> Expr {
    let input_id = ctx.fresh_local();
    let mut body = Vec::new();
    let fields = match ir {
        ZodSchemaIr::Object(fields) => fields,
    };

    let not_object = compare(
        CompareOp::Ne,
        Expr::TypeOf(Box::new(Expr::LocalGet(input_id))),
        Expr::String("object".to_string()),
    );
    let null_input = compare(CompareOp::Eq, Expr::LocalGet(input_id), Expr::Null);
    let array_input = Expr::ArrayIsArray(Box::new(Expr::LocalGet(input_id)));
    body.push(return_invalid_if(
        logical(
            LogicalOp::Or,
            logical(LogicalOp::Or, not_object, null_input),
            array_input,
        ),
        invalid_id,
    ));

    let mut output_fields = Vec::with_capacity(fields.len());
    for field in fields {
        let field_id = ctx.fresh_local();
        body.push(Stmt::Let {
            id: field_id,
            name: format!("__zod_{}", field.key),
            ty: Type::Any,
            mutable: false,
            init: Some(property(Expr::LocalGet(input_id), field.key.clone())),
        });

        let type_name = match field.value {
            ZodValueIr::String => "string",
            ZodValueIr::Number { .. } => "number",
            ZodValueIr::Boolean => "boolean",
        };
        let wrong_type = compare(
            CompareOp::Ne,
            Expr::TypeOf(Box::new(Expr::LocalGet(field_id))),
            Expr::String(type_name.to_string()),
        );
        let invalid_type = match &field.value {
            ZodValueIr::Number { .. } => logical(
                LogicalOp::Or,
                wrong_type,
                not(Expr::NumberIsFinite(Box::new(Expr::LocalGet(field_id)))),
            ),
            _ => wrong_type,
        };
        body.push(return_invalid_if(invalid_type, invalid_id));

        if let ZodValueIr::Number {
            integer,
            minimum,
            maximum,
        } = field.value
        {
            if integer {
                body.push(return_invalid_if(
                    not(Expr::NumberIsInteger(Box::new(Expr::LocalGet(field_id)))),
                    invalid_id,
                ));
            }
            if let Some(minimum) = minimum {
                body.push(return_invalid_if(
                    compare(
                        CompareOp::Lt,
                        Expr::LocalGet(field_id),
                        Expr::Number(minimum),
                    ),
                    invalid_id,
                ));
            }
            if let Some(maximum) = maximum {
                body.push(return_invalid_if(
                    compare(
                        CompareOp::Gt,
                        Expr::LocalGet(field_id),
                        Expr::Number(maximum),
                    ),
                    invalid_id,
                ));
            }
        }

        output_fields.push((field.key.clone(), Expr::LocalGet(field_id)));
    }
    body.push(Stmt::Return(Some(Expr::Object(output_fields))));

    let func_id = ctx.fresh_func();
    ctx.closure_display_names
        .insert(func_id, "__perry_zod_native_parser".to_string());
    closure(
        func_id,
        vec![param(input_id, "input")],
        body,
        vec![invalid_id],
        ctx.current_strict_mode(),
    )
}

fn lower_compiled_schema_install(
    ctx: &mut LoweringContext,
    runtime_namespace: Expr,
    runtime_schema: Expr,
    runtime_options: Expr,
    ir: &ZodSchemaIr,
) -> Expr {
    let strict = ctx.current_strict_mode();
    let namespace_id = ctx.fresh_local();
    let source_id = ctx.fresh_local();
    let options_id = ctx.fresh_local();
    let invalid_id = ctx.fresh_local();
    let parser_id = ctx.fresh_local();

    let parser = lower_native_parser(ctx, ir, invalid_id);

    let outer_body = vec![
        Stmt::If {
            condition: compare(
                CompareOp::Ne,
                Expr::TypeOf(Box::new(property(
                    Expr::LocalGet(namespace_id),
                    "compileFromParser",
                ))),
                Expr::String("function".to_string()),
            ),
            then_branch: vec![Stmt::Return(Some(call(
                property(Expr::LocalGet(namespace_id), "compile"),
                vec![Expr::LocalGet(source_id), Expr::LocalGet(options_id)],
            )))],
            else_branch: None,
        },
        Stmt::Let {
            id: invalid_id,
            name: "__zod_invalid".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(property(Expr::LocalGet(namespace_id), "INVALID")),
        },
        Stmt::Let {
            id: parser_id,
            name: "__zod_native_parser".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(parser),
        },
        Stmt::Return(Some(call(
            property(Expr::LocalGet(namespace_id), "compileFromParser"),
            vec![Expr::LocalGet(source_id), Expr::LocalGet(parser_id)],
        ))),
    ];
    let outer_func = ctx.fresh_func();
    ctx.closure_display_names
        .insert(outer_func, "__perry_zod_install_parser".to_string());
    let outer = closure(
        outer_func,
        vec![
            param(namespace_id, "zod"),
            param(source_id, "schema"),
            param(options_id, "options"),
        ],
        outer_body,
        Vec::new(),
        strict,
    );
    call(
        outer,
        vec![runtime_namespace, runtime_schema, runtime_options],
    )
}

fn strict_options_only(call: &ast::CallExpr) -> bool {
    if call.args.len() == 1 {
        return true;
    }
    if call.args.len() != 2 || call.args[1].spread.is_some() {
        return false;
    }
    let ast::Expr::Object(options) = peel_expr(&call.args[1].expr) else {
        return false;
    };
    options.props.iter().all(|property| {
        let ast::PropOrSpread::Prop(property) = property else {
            return false;
        };
        let ast::Prop::KeyValue(kv) = property.as_ref() else {
            return false;
        };
        object_key(&kv.key).as_deref() == Some("strict")
            && matches!(peel_expr(&kv.value), ast::Expr::Lit(ast::Lit::Bool(_)))
    })
}

pub(crate) fn try_lower_zod_compile(
    ctx: &mut LoweringContext,
    call_expr: &ast::CallExpr,
) -> Result<Option<Expr>> {
    if call_expr.args.is_empty()
        || call_expr.args[0].spread.is_some()
        || !strict_options_only(call_expr)
    {
        return Ok(None);
    }
    let Some((receiver, method)) = call_member(call_expr) else {
        return Ok(None);
    };
    if method != "compile" || !is_zod_namespace(ctx, receiver) {
        return Ok(None);
    }

    let schema_expr = peel_expr(&call_expr.args[0].expr);
    let ir = match schema_expr {
        ast::Expr::Ident(id) => ctx
            .lookup_local(id.sym.as_ref())
            .and_then(|local| ctx.zod_schema_irs.get(&local))
            .cloned(),
        _ => extract_schema_ir(ctx, schema_expr),
    };
    let Some(ir) = ir else {
        return Ok(None);
    };
    let runtime_namespace = lower_expr(ctx, receiver)?;
    let runtime_schema = lower_expr(ctx, schema_expr)?;
    let runtime_options = match call_expr.args.get(1) {
        Some(options) => lower_expr(ctx, &options.expr)?,
        None => Expr::Undefined,
    };
    Ok(Some(lower_compiled_schema_install(
        ctx,
        runtime_namespace,
        runtime_schema,
        runtime_options,
        &ir,
    )))
}

#[cfg(test)]
mod tests {
    #[test]
    fn static_zod_compile_lowers_to_named_native_validator_hir() {
        let source = r#"
            import * as z from "zod";
            const Player = z.object({
                name: z.string(),
                score: z.number().int().min(0).max(100),
                active: z.boolean(),
            });
            export const CompiledPlayer = z.compile(Player, { strict: true });
        "#;
        let parsed = perry_parser::parse_typescript(source, "zod-aot.ts").expect("source parses");
        let hir = crate::lower_module(&parsed, "zod-aot", "zod-aot.ts").expect("source lowers");

        for expected in ["__perry_zod_install_parser", "__perry_zod_native_parser"] {
            assert!(
                hir.closure_display_names
                    .values()
                    .any(|name| name == expected),
                "missing {expected}: {:#?}",
                hir.closure_display_names
            );
        }

        let dump = format!("{hir:#?}");
        assert!(dump.contains("NumberIsFinite"), "{dump}");
        assert!(dump.contains("NumberIsInteger"), "{dump}");
        assert!(dump.contains("compileFromParser"), "{dump}");
        assert!(dump.contains("INVALID"), "{dump}");
        assert!(!dump.contains("__perry_zod_wrapped_run"), "{dump}");
        assert!(dump.contains("0.0"), "{dump}");
        assert!(dump.contains("100.0"), "{dump}");
    }

    #[test]
    fn unsupported_schema_keeps_the_regular_zod_call() {
        let source = r#"
            import * as z from "zod";
            const Dynamic = z.array(z.string());
            export const Compiled = z.compile(Dynamic, { strict: true });
        "#;
        let parsed =
            perry_parser::parse_typescript(source, "zod-fallback.ts").expect("source parses");
        let hir =
            crate::lower_module(&parsed, "zod-fallback", "zod-fallback.ts").expect("source lowers");

        assert!(
            hir.closure_display_names
                .values()
                .all(|name| !name.starts_with("__perry_zod_")),
            "unsupported schemas must not be partly specialized: {:#?}",
            hir.closure_display_names
        );
    }

    #[test]
    fn mutable_schema_keeps_the_regular_zod_call() {
        let source = r#"
            import * as z from "zod";
            let Player = z.object({ name: z.string() });
            Player = z.object({ name: z.string() });
            export const Compiled = z.compile(Player);
        "#;
        let parsed =
            perry_parser::parse_typescript(source, "zod-mutable.ts").expect("source parses");
        let hir =
            crate::lower_module(&parsed, "zod-mutable", "zod-mutable.ts").expect("source lowers");

        assert!(
            hir.closure_display_names
                .values()
                .all(|name| !name.starts_with("__perry_zod_")),
            "mutable schemas must not be specialized: {:#?}",
            hir.closure_display_names
        );
    }

    #[test]
    fn shadowed_zod_namespace_keeps_the_regular_call() {
        let source = r#"
            import * as z from "zod";
            function compileLocal() {
                const z = makeLocalSchemaLibrary();
                const Player = z.object({ name: z.string() });
                return z.compile(Player);
            }
        "#;
        let parsed =
            perry_parser::parse_typescript(source, "zod-shadow.ts").expect("source parses");
        let hir =
            crate::lower_module(&parsed, "zod-shadow", "zod-shadow.ts").expect("source lowers");

        assert!(
            hir.closure_display_names
                .values()
                .all(|name| !name.starts_with("__perry_zod_")),
            "shadowed namespace bindings must not be specialized: {:#?}",
            hir.closure_display_names
        );
    }

    #[test]
    fn duplicate_object_keys_keep_the_regular_zod_call() {
        let source = r#"
            import * as z from "zod";
            export const Compiled = z.compile(z.object({
                name: z.string(),
                name: z.string(),
            }));
        "#;
        let parsed =
            perry_parser::parse_typescript(source, "zod-duplicate.ts").expect("source parses");
        let hir = crate::lower_module(&parsed, "zod-duplicate", "zod-duplicate.ts")
            .expect("source lowers");

        assert!(
            hir.closure_display_names
                .values()
                .all(|name| !name.starts_with("__perry_zod_")),
            "duplicate keys must not be specialized: {:#?}",
            hir.closure_display_names
        );
    }
}
