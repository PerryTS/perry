//! JSX lowering.
//!
//! Contains functions for lowering JSX elements, fragments, attributes,
//! and children into HIR expressions.

use anyhow::Result;
use perry_types::Type;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::{lower_expr, LoweringContext};

pub(crate) fn lower_jsx_element(ctx: &mut LoweringContext, jsx: &ast::JSXElement) -> Result<Expr> {
    let type_expr = lower_jsx_element_name(ctx, &jsx.opening.name)?;

    let mut props_fields: Vec<(String, Expr)> = Vec::new();
    for attr in &jsx.opening.attrs {
        match attr {
            ast::JSXAttrOrSpread::JSXAttr(jsx_attr) => {
                let attr_name = match &jsx_attr.name {
                    ast::JSXAttrName::Ident(id) => id.sym.to_string(),
                    ast::JSXAttrName::JSXNamespacedName(ns) => {
                        format!("{}:{}", ns.ns.sym, ns.name.sym)
                    }
                };
                // 'key' is handled by React internally, not passed as a prop
                if attr_name == "key" {
                    continue;
                }
                let attr_val = match &jsx_attr.value {
                    None => Expr::Bool(true), // Boolean attribute: <input disabled />
                    Some(val) => lower_jsx_attr_value(ctx, val)?,
                };
                props_fields.push((attr_name, attr_val));
            }
            ast::JSXAttrOrSpread::SpreadElement(spread) => {
                // Spread attributes ({...obj}) are not yet representable in HIR Object.
                // Evaluate for side effects but don't propagate into props.
                let _ = lower_expr(ctx, &spread.expr);
            }
        }
    }

    let mut children: Vec<Expr> = Vec::new();
    for child in &jsx.children {
        if let Some(child_expr) = lower_jsx_child(ctx, child)? {
            children.push(child_expr);
        }
    }

    // Use the original exported names (not the local __jsx/__jsxs aliases) so Perry
    // generates the correct wrapper symbol names: __wrapper_jsx / __wrapper_jsxs.
    let func_name = if children.len() > 1 { "jsxs" } else { "jsx" };
    match children.len() {
        0 => {}
        1 => {
            props_fields.push(("children".to_string(), children.remove(0)));
        }
        _ => {
            props_fields.push(("children".to_string(), Expr::Array(children)));
        }
    }

    let props_expr = if props_fields.is_empty() {
        Expr::Null
    } else {
        Expr::Object(props_fields)
    };

    Ok(Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: func_name.to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }),
        args: vec![type_expr, props_expr],
        type_args: Vec::new(),
    })
}

/// Lower a JSX fragment (`<>…</>`) to a `jsx(Fragment, { children })` call.
pub(crate) fn lower_jsx_fragment(
    ctx: &mut LoweringContext,
    jsx: &ast::JSXFragment,
) -> Result<Expr> {
    let mut children: Vec<Expr> = Vec::new();
    for child in &jsx.children {
        if let Some(child_expr) = lower_jsx_child(ctx, child)? {
            children.push(child_expr);
        }
    }

    // Use original exported names for correct wrapper symbol generation.
    let func_name = if children.len() > 1 { "jsxs" } else { "jsx" };
    let mut props_fields: Vec<(String, Expr)> = Vec::new();
    match children.len() {
        0 => {}
        1 => {
            props_fields.push(("children".to_string(), children.remove(0)));
        }
        _ => {
            props_fields.push(("children".to_string(), Expr::Array(children)));
        }
    }

    let props_expr = if props_fields.is_empty() {
        Expr::Null
    } else {
        Expr::Object(props_fields)
    };

    Ok(Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: func_name.to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }),
        // Fragment marker: inline "__Fragment" string. perry-react's jsx() checks
        // `type === "__Fragment"` to detect fragment elements.
        args: vec![Expr::String("__Fragment".to_string()), props_expr],
        type_args: Vec::new(),
    })
}

/// Lower a JSX element name to an HIR expression.
/// Lowercase tag names (HTML intrinsics) become string literals.
/// Uppercase tag names (components) are looked up as identifiers.
pub(crate) fn lower_jsx_element_name(
    ctx: &mut LoweringContext,
    name: &ast::JSXElementName,
) -> Result<Expr> {
    match name {
        ast::JSXElementName::Ident(ident) => {
            let sym = ident.sym.as_ref();
            // Convention: lowercase first char = HTML intrinsic element
            let first_char = sym.chars().next().unwrap_or('a');
            if first_char.is_lowercase() || first_char == '_' {
                Ok(Expr::String(sym.to_string()))
            } else {
                // Component reference - resolve identifier in scope
                let n = sym.to_string();
                if let Some(id) = ctx.lookup_local(&n) {
                    Ok(Expr::LocalGet(id))
                } else if let Some(id) = ctx.lookup_func(&n) {
                    Ok(Expr::FuncRef(id))
                } else if let Some((module_name, method_name)) = ctx.lookup_native_module(&n) {
                    // Native-module-imported JSX intrinsic (e.g. `<Box>` /
                    // `<Text>` from `perry/tui`). Tag the ExternFuncRef
                    // with a sentinel name
                    // `__perry_jsx_intrinsic::<module>::<method>__` so the
                    // codegen's `jsx`/`jsxs` arm can recognise it as a
                    // built-in intrinsic and rewrite the runtime `js_jsx`
                    // call into a direct widget-builder call. Including
                    // the module name in the sentinel ensures the
                    // rewriter only triggers when the JSX-named
                    // identifier resolves to a known native module —
                    // never a user-defined `Box` from elsewhere
                    // (issue #689).
                    let module = module_name.to_string();
                    let method = method_name.unwrap_or(&n).to_string();
                    let sentinel = format!("__perry_jsx_intrinsic::{module}::{method}__");
                    Ok(Expr::ExternFuncRef {
                        name: sentinel,
                        param_types: Vec::new(),
                        return_type: Type::Any,
                    })
                } else if let Some(orig) = ctx.lookup_imported_func(&n) {
                    Ok(Expr::ExternFuncRef {
                        name: orig.to_string(),
                        param_types: Vec::new(),
                        return_type: Type::Any,
                    })
                } else {
                    // Unknown identifier – treat as an extern reference
                    Ok(Expr::ExternFuncRef {
                        name: n,
                        param_types: Vec::new(),
                        return_type: Type::Any,
                    })
                }
            }
        }
        ast::JSXElementName::JSXMemberExpr(member) => {
            // e.g. React.Fragment → PropertyGet on the namespace
            let obj_expr = lower_jsx_object(ctx, &member.obj)?;
            Ok(Expr::PropertyGet {
                object: Box::new(obj_expr),
                property: member.prop.sym.to_string(),
            })
        }
        ast::JSXElementName::JSXNamespacedName(ns) => {
            // e.g. svg:circle → treated as a plain string for now
            Ok(Expr::String(format!("{}:{}", ns.ns.sym, ns.name.sym)))
        }
    }
}

/// Lower a JSX member-expression object (the left-hand side of `Foo.Bar.Baz`).
pub(crate) fn lower_jsx_object(ctx: &mut LoweringContext, obj: &ast::JSXObject) -> Result<Expr> {
    match obj {
        ast::JSXObject::Ident(ident) => {
            let n = ident.sym.to_string();
            if let Some(id) = ctx.lookup_local(&n) {
                Ok(Expr::LocalGet(id))
            } else if let Some(id) = ctx.lookup_func(&n) {
                Ok(Expr::FuncRef(id))
            } else if let Some(orig) = ctx.lookup_imported_func(&n) {
                Ok(Expr::ExternFuncRef {
                    name: orig.to_string(),
                    param_types: Vec::new(),
                    return_type: Type::Any,
                })
            } else {
                Ok(Expr::ExternFuncRef {
                    name: n,
                    param_types: Vec::new(),
                    return_type: Type::Any,
                })
            }
        }
        ast::JSXObject::JSXMemberExpr(member) => {
            let obj_expr = lower_jsx_object(ctx, &member.obj)?;
            Ok(Expr::PropertyGet {
                object: Box::new(obj_expr),
                property: member.prop.sym.to_string(),
            })
        }
    }
}

/// Lower a JSX attribute value to an HIR expression.
pub(crate) fn lower_jsx_attr_value(
    ctx: &mut LoweringContext,
    value: &ast::JSXAttrValue,
) -> Result<Expr> {
    match value {
        ast::JSXAttrValue::Str(s) => Ok(Expr::String(s.value.as_str().unwrap_or("").to_string())),
        ast::JSXAttrValue::JSXExprContainer(container) => match &container.expr {
            ast::JSXExpr::JSXEmptyExpr(_) => Ok(Expr::Undefined),
            ast::JSXExpr::Expr(expr) => lower_expr(ctx, expr),
        },
        ast::JSXAttrValue::JSXElement(elem) => lower_jsx_element(ctx, elem),
        ast::JSXAttrValue::JSXFragment(frag) => lower_jsx_fragment(ctx, frag),
    }
}

/// Lower a JSX child node to an optional HIR expression.
/// Returns `None` for whitespace-only text nodes (they are elided, matching React's behaviour).
pub(crate) fn lower_jsx_child(
    ctx: &mut LoweringContext,
    child: &ast::JSXElementChild,
) -> Result<Option<Expr>> {
    match child {
        ast::JSXElementChild::JSXText(text) => {
            let normalized = normalize_jsx_text(text.value.as_ref());
            if normalized.is_empty() {
                Ok(None)
            } else {
                Ok(Some(Expr::String(normalized)))
            }
        }
        ast::JSXElementChild::JSXExprContainer(container) => match &container.expr {
            ast::JSXExpr::JSXEmptyExpr(_) => Ok(None),
            ast::JSXExpr::Expr(expr) => lower_expr(ctx, expr).map(Some),
        },
        ast::JSXElementChild::JSXSpreadChild(spread) => lower_expr(ctx, &spread.expr).map(Some),
        ast::JSXElementChild::JSXElement(elem) => lower_jsx_element(ctx, elem).map(Some),
        ast::JSXElementChild::JSXFragment(frag) => lower_jsx_fragment(ctx, frag).map(Some),
    }
}

/// Normalize JSX text content following React's whitespace rules:
/// - Split by newlines, trim each line, filter empty lines, join with a space.
pub(crate) fn normalize_jsx_text(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.len() == 1 {
        return text.trim().to_string();
    }
    lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
