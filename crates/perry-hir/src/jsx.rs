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

    // Use Perry's built-in extern names so codegen can route TSX straight to
    // the native `js_jsx` / `js_jsxs` runtime adapter.
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

    // #4950: a module that default-imports the npm `react` package gets
    // REACT element semantics — `React.createElement(type, props)` returns
    // an element OBJECT and the reconciler calls the component later, with
    // the hooks dispatcher installed. Perry's native `jsx` adapter calls
    // function components eagerly (SSR-to-HTML, the hono path), which under
    // ink/react-reconciler ran `<Text>`'s `useContext` outside any render
    // and died on React's "Invalid hook call" null-dispatcher TypeError.
    // (`createElement` reads `props.children` the same way the jsx-runtime
    // does, so folding children into props is shared between both paths.)
    if let Some(react_element_call) = react_create_element_call(ctx, type_expr.clone(), &props_expr)
    {
        return Ok(react_element_call);
    }

    Ok(Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: func_name.to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }),
        args: vec![type_expr, props_expr],
        type_args: Vec::new(),
        byte_offset: 0,
    })
}

/// #4950: build `<ReactLocal>.createElement(type, props)` when the module
/// default-imports the npm `react` package. Returns `None` when no react
/// binding is in scope (Perry's native `js_jsx` semantics apply).
fn react_create_element_call(
    ctx: &mut LoweringContext,
    type_expr: Expr,
    props_expr: &Expr,
) -> Option<Expr> {
    let react_local = ctx.react_default_import_local.clone()?;
    // Resolve the react binding the same way ordinary ident lowering would:
    // a shadowing local wins, otherwise the imported-function registration.
    let object = if let Some(id) = ctx.lookup_local(&react_local) {
        Expr::LocalGet(id)
    } else if let Some(orig) = ctx.lookup_imported_func(&react_local) {
        Expr::ExternFuncRef {
            name: orig.to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }
    } else {
        return None;
    };
    Some(Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(object),
            property: "createElement".to_string(),
        }),
        args: vec![type_expr, props_expr.clone()],
        type_args: Vec::new(),
        byte_offset: 0,
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

    // Use Perry's built-in extern names for the same runtime routing as
    // ordinary JSX elements.
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

    // #4950: react-mode fragments are `React.createElement(React.Fragment,
    // props)` — see `react_create_element_call`.
    if let Some(react_local) = ctx.react_default_import_local.clone() {
        let fragment_type = Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(if let Some(id) = ctx.lookup_local(&react_local) {
                Expr::LocalGet(id)
            } else {
                Expr::ExternFuncRef {
                    name: ctx
                        .lookup_imported_func(&react_local)
                        .unwrap_or(&react_local)
                        .to_string(),
                    param_types: Vec::new(),
                    return_type: Type::Any,
                }
            }),
            property: "Fragment".to_string(),
        };
        if let Some(call) = react_create_element_call(ctx, fragment_type, &props_expr) {
            return Ok(call);
        }
    }

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
        byte_offset: 0,
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
                byte_offset: 0,
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
                byte_offset: 0,
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

/// Normalize JSX text content following React/Babel's whitespace rules
/// (`cleanJSXElementLiteralChild`): leading whitespace is trimmed only on
/// non-first lines, trailing only on non-last lines, tabs become spaces, and
/// non-empty lines are joined with a single space. Crucially a single-line
/// text node is preserved verbatim — so the trailing space in
/// `<h1>hello, {name}</h1>` and the gap in `{"x"} {"y"}` survive (a blanket
/// `trim()` used to drop them, giving `hello,Perry` / `xy`) — #1653.
pub(crate) fn normalize_jsx_text(text: &str) -> String {
    let lines: Vec<&str> = text.split(['\r', '\n']).collect();
    // Index of the last line containing a non-whitespace char (Babel seeds
    // this at 0, so an all-whitespace single space stays a single space).
    let last_non_empty = lines
        .iter()
        .rposition(|l| l.contains(|c: char| c != ' ' && c != '\t'))
        .unwrap_or(0);
    let n = lines.len();
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        let mut seg = line.replace('\t', " ");
        if i != 0 {
            seg = seg.trim_start_matches(' ').to_string();
        }
        if i != n - 1 {
            seg = seg.trim_end_matches(' ').to_string();
        }
        if !seg.is_empty() {
            if i != last_non_empty {
                seg.push(' ');
            }
            out.push_str(&seg);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsx_uses_react_create_element_when_react_default_import_present() {
        // #4950: with `import React from 'react'` in scope, JSX must build
        // elements via `React.createElement` (reconciler-controlled component
        // invocation), never Perry's eager `js_jsx` adapter.
        let mut ctx = LoweringContext::new("test.tsx");
        ctx.react_default_import_local = Some("React".to_string());
        ctx.register_imported_func("React".to_string(), "React".to_string());
        let call =
            react_create_element_call(&mut ctx, Expr::String("div".to_string()), &Expr::Null)
                .expect("react mode must produce a createElement call");
        match call {
            Expr::Call { callee, args, .. } => {
                match *callee {
                    Expr::PropertyGet { property, .. } => assert_eq!(property, "createElement"),
                    other => panic!("expected PropertyGet callee, got {other:?}"),
                }
                assert_eq!(args.len(), 2);
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn jsx_keeps_native_adapter_without_react_import() {
        let mut ctx = LoweringContext::new("test.tsx");
        assert!(
            react_create_element_call(&mut ctx, Expr::String("div".to_string()), &Expr::Null)
                .is_none(),
            "without a react import the native js_jsx adapter must stay in effect"
        );
    }
}
