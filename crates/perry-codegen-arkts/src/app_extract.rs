// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// Find the first top-level `App({body: <expr>})` call in `module.init`,
/// **return its body by-value**, and replace the entire statement with a
/// no-op `Stmt::Expr(Expr::Number(0.0))`. Other statements are untouched
/// so logic before/after `App(...)` still runs in `perryEntry.run()`.
pub(crate) fn find_and_strip_app(init: &mut [Stmt], classes: &[Class]) -> Option<Expr> {
    for stmt in init.iter_mut() {
        if let Stmt::Expr(Expr::NativeMethodCall {
            module: m,
            method,
            object: None,
            args,
            ..
        }) = stmt
        {
            if m == "perry/ui" && method == "App" && args.len() == 1 {
                let body = extract_body_field(&mut args[0], classes);
                if body.is_some() {
                    *stmt = Stmt::Expr(Expr::Number(0.0));
                    return body;
                }
            }
        }
    }
    None
}

/// Pull out the `body:` field's expression from either a plain
/// `Expr::Object` or a `__AnonShape_*` `Expr::New`. Returns the body by
/// value (cloned for the New case since we can't move out of args[idx]
/// without disturbing the rest of the args array, but the strip below
/// throws the whole call away anyway).
pub(crate) fn extract_body_field(arg: &mut Expr, classes: &[Class]) -> Option<Expr> {
    match arg {
        Expr::Object(props) => {
            let idx = props.iter().position(|(k, _)| k == "body")?;
            let (_, body) = props.remove(idx);
            Some(body)
        }
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            let class = classes.iter().find(|c| &c.name == class_name)?;
            let body_idx = class.fields.iter().position(|f| f.name == "body")?;
            args.get(body_idx).cloned()
        }
        _ => None,
    }
}

/// Extract the `name` field from a route spec object — handles open
/// `Expr::Object` and Perry's closed-shape `Expr::New { __AnonShape_* }`.
pub(crate) fn extract_route_name(spec: &Expr, classes: &[Class]) -> Option<String> {
    let pairs: Vec<(String, Expr)> = match spec {
        Expr::Object(props) => props.clone(),
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            classes.iter().find(|c| &c.name == class_name).map(|cls| {
                cls.fields
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| args.get(i).map(|a| (f.name.clone(), a.clone())))
                    .collect()
            })?
        }
        _ => return None,
    };
    pairs
        .into_iter()
        .find(|(k, _)| k == "name")
        .and_then(|(_, v)| match v {
            Expr::String(s) => Some(s),
            _ => None,
        })
}

/// Extract the `body` field from a route spec object.
pub(crate) fn extract_route_body(spec: &Expr, classes: &[Class]) -> Option<Expr> {
    let pairs: Vec<(String, Expr)> = match spec {
        Expr::Object(props) => props.clone(),
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            classes.iter().find(|c| &c.name == class_name).map(|cls| {
                cls.fields
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| args.get(i).map(|a| (f.name.clone(), a.clone())))
                    .collect()
            })?
        }
        _ => return None,
    };
    pairs.into_iter().find(|(k, _)| k == "body").map(|(_, v)| v)
}
