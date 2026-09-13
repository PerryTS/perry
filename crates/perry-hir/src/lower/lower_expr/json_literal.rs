//! Keep large data literals out of generated constructor/store sequences (#10151).
//!
//! Defines have already been substituted and their `typeof` guards folded by
//! the parser. Use the same intrinsic as JSON imports (#8418), at the original
//! evaluation site: every evaluation still creates a fresh object/array, and
//! an untaken branch does not parse anything. No user `JSON` binding is read.

use swc_ecma_ast as ast;

use crate::ir::Expr;

const MIN_NODES: usize = 1024;
const MIN_TEXT_BYTES: usize = 64 * 1024;
// Parsed records lose the static shapes used by ordinary literal lowering.
// Reserve that tradeoff for codegen-sized data: the record-array probe in
// benchmarks/large_json_literals measures the LLVM cliff independently of
// function instruction budgets. Keep mid-size typed records on the fast read path.
const CODEGEN_NODES: usize = 24 * 1024;
const CODEGEN_TEXT_BYTES: usize = 1024 * 1024;
// Bound speculative subtree walks, including on deeply nested non-JSON input.
const MAX_DEPTH: usize = 128;

pub(super) fn lower_large_json_literal(expr: &ast::Expr) -> Option<Expr> {
    if !is_large(expr) {
        return None;
    }
    let mut text = Vec::new();
    serialize(expr, &mut text, 0)?;
    Some(Expr::JsonParse(Box::new(Expr::String(
        String::from_utf8(text).ok()?,
    ))))
}

/// Cheap, bounded probe. Count AST value nodes and UTF-8 key/string bytes;
/// whitespace and source spans (which defines inherit from another file) are
/// irrelevant. Stop as soon as either threshold is reached, then validate and
/// serialize the whole candidate exactly once.
fn is_large(expr: &ast::Expr) -> bool {
    let (min_nodes, min_bytes) = if is_primitive_array(expr) {
        (MIN_NODES, MIN_TEXT_BYTES)
    } else {
        (CODEGEN_NODES, CODEGEN_TEXT_BYTES)
    };
    let mut pending = vec![(expr, 0)];
    let (mut nodes, mut bytes) = (0, 0);
    while let Some((expr, depth)) = pending.pop() {
        if depth > MAX_DEPTH {
            return false;
        }
        nodes += 1;
        match expr {
            ast::Expr::Object(object) => {
                for prop in &object.props {
                    let ast::PropOrSpread::Prop(prop) = prop else {
                        return false;
                    };
                    let ast::Prop::KeyValue(kv) = prop.as_ref() else {
                        return false;
                    };
                    bytes += match &kv.key {
                        ast::PropName::Ident(key) => key.sym.len(),
                        ast::PropName::Str(key) => key.value.len(),
                        ast::PropName::Num(_) => 0,
                        _ => return false,
                    };
                    pending.push((&kv.value, depth + 1));
                }
            }
            ast::Expr::Array(array) => {
                for elem in &array.elems {
                    let Some(elem) = elem else { return false };
                    if elem.spread.is_some() {
                        return false;
                    }
                    pending.push((&elem.expr, depth + 1));
                }
            }
            ast::Expr::Paren(paren) => pending.push((&paren.expr, depth + 1)),
            ast::Expr::Unary(unary)
                if matches!(unary.op, ast::UnaryOp::Minus | ast::UnaryOp::Plus) =>
            {
                pending.push((&unary.arg, depth + 1));
            }
            ast::Expr::Lit(ast::Lit::Str(value)) => bytes += value.value.len(),
            ast::Expr::Lit(ast::Lit::Num(_) | ast::Lit::Bool(_) | ast::Lit::Null(_)) => {}
            _ => return false,
        }
        if nodes >= min_nodes || bytes >= min_bytes {
            return true;
        }
    }
    false
}

/// Only flat arrays get the lower threshold. In particular, an array of
/// records must not lose its statically known property layout at this size.
fn is_primitive_array(expr: &ast::Expr) -> bool {
    let ast::Expr::Array(array) = expr else {
        return false;
    };
    array.elems.iter().all(|elem| {
        elem.as_ref()
            .is_some_and(|elem| elem.spread.is_none() && is_primitive(&elem.expr, 0))
    })
}

fn is_primitive(expr: &ast::Expr, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    match expr {
        ast::Expr::Lit(
            ast::Lit::Num(_) | ast::Lit::Str(_) | ast::Lit::Bool(_) | ast::Lit::Null(_),
        ) => true,
        ast::Expr::Paren(paren) => is_primitive(&paren.expr, depth + 1),
        ast::Expr::Unary(unary) => {
            matches!(unary.op, ast::UnaryOp::Minus | ast::UnaryOp::Plus)
                && matches!(unary.arg.as_ref(), ast::Expr::Lit(ast::Lit::Num(_)))
        }
        _ => false,
    }
}

fn string(text: &mut Vec<u8>, value: &str) -> Option<()> {
    serde_json::to_writer(text, value).ok()
}

fn number(text: &mut Vec<u8>, value: f64) -> Option<()> {
    // JSON cannot represent Infinity/NaN. serde_json would silently emit null.
    if !value.is_finite() {
        return None;
    }
    // Serialize the parsed f64, preserving negative zero and numeric semantics
    // for JS spellings such as hex literals and unary plus.
    serde_json::to_writer(text, &value).ok()
}

fn serialize(expr: &ast::Expr, text: &mut Vec<u8>, depth: usize) -> Option<()> {
    if depth > MAX_DEPTH {
        return None;
    }
    match expr {
        ast::Expr::Object(object) => {
            text.push(b'{');
            for (i, prop) in object.props.iter().enumerate() {
                let ast::PropOrSpread::Prop(prop) = prop else {
                    return None;
                };
                let ast::Prop::KeyValue(kv) = prop.as_ref() else {
                    return None;
                };
                let key = match &kv.key {
                    ast::PropName::Ident(key) => key.sym.to_string(),
                    ast::PropName::Str(key) => key.value.as_str()?.to_owned(),
                    ast::PropName::Num(key) => crate::lower::number_to_js_key(key.value),
                    _ => return None,
                };
                // Object literals set [[Prototype]] here; JSON.parse creates an
                // own data property. Leave that expression on ordinary lowering.
                if key == "__proto__" {
                    return None;
                }
                if i != 0 {
                    text.push(b',');
                }
                string(text, &key)?;
                text.push(b':');
                serialize(&kv.value, text, depth + 1)?;
            }
            text.push(b'}');
        }
        ast::Expr::Array(array) => {
            text.push(b'[');
            for (i, elem) in array.elems.iter().enumerate() {
                let elem = elem.as_ref()?;
                if elem.spread.is_some() {
                    return None;
                }
                if i != 0 {
                    text.push(b',');
                }
                serialize(&elem.expr, text, depth + 1)?;
            }
            text.push(b']');
        }
        ast::Expr::Paren(paren) => serialize(&paren.expr, text, depth + 1)?,
        ast::Expr::Unary(unary) => {
            let ast::Expr::Lit(ast::Lit::Num(value)) = unary.arg.as_ref() else {
                return None;
            };
            number(
                text,
                match unary.op {
                    ast::UnaryOp::Minus => -value.value,
                    ast::UnaryOp::Plus => value.value,
                    _ => return None,
                },
            )?;
        }
        ast::Expr::Lit(ast::Lit::Str(value)) => string(text, value.value.as_str()?)?,
        ast::Expr::Lit(ast::Lit::Num(value)) => number(text, value.value)?,
        ast::Expr::Lit(ast::Lit::Bool(value)) => {
            text.extend_from_slice(if value.value { b"true" } else { b"false" })
        }
        ast::Expr::Lit(ast::Lit::Null(_)) => text.extend_from_slice(b"null"),
        _ => return None,
    }
    Some(())
}

#[cfg(test)]
mod tests;
