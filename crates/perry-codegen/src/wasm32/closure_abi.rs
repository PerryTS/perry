//! Give every closure body the runtime's view of its first parameter.
//!
//! Codegen defines a closure body as `(i64 %this_closure, i64 %js_this,
//! double …)` (`perry_abi::JS_BODY_*`): the closure pointer travels as an
//! `i64` like every other address it carries. The runtime invokes those
//! bodies through `extern "C" fn(*const ClosureHeader, JsThis, f64 …)` (and
//! defines its own native-closure bodies that way), so on wasm32 the first
//! slot is an `i32` pointer; the receiver is a `u64` on both sides. A native C ABI
//! does not care; a wasm `call_indirect` must match exactly, and traps.
//!
//! This pass retypes each such definition to take `ptr %this_closure.ptr`
//! and recovers the `i64` the body was written against in its entry block, so
//! the body text is otherwise untouched. The retyped signatures are returned
//! so direct calls to these bodies from the same module can be routed through
//! the same adapters as runtime calls ([`super::abi_adapt`]).

use std::collections::HashMap;

/// A retyped body: its IR return type and parameter types, after the change.
pub(super) struct Retyped {
    pub(super) ret: String,
    pub(super) params: Vec<String>,
}

/// Retype every `define` whose first parameter is `i64 %this_closure`.
pub(super) fn retype_closure_bodies(ir: &str) -> (String, HashMap<String, Retyped>) {
    let mut out = String::with_capacity(ir.len() + 1024);
    let mut retyped = HashMap::new();
    // Set after a retyped `define` line: the `ptrtoint` still has to be
    // placed, right after the entry block's label if it has one.
    let mut pending: Option<String> = None;
    for line in ir.split_inclusive('\n') {
        if let Some(recover) = pending.take() {
            let body = line.trim_end();
            let is_label = !body.starts_with(' ')
                && body
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim_end()
                    .ends_with(':');
            if is_label {
                out.push_str(line);
                out.push_str(&recover);
                continue;
            }
            out.push_str(&recover);
        }
        if let Some((name, ret, params, param)) = closure_define(line) {
            out.push_str(&line.replacen(&format!("(i64 {param}"), &format!("(ptr {param}.ptr"), 1));
            let mut params = params;
            params[0] = "ptr".to_string();
            retyped.insert(name, Retyped { ret, params });
            pending = Some(format!("  {param} = ptrtoint ptr {param}.ptr to i64\n"));
            continue;
        }
        out.push_str(line);
    }
    (out, retyped)
}

/// `define [linkage …] <ret> @name(i64 %this_closure, <ty> %x, …) … {`
/// → (name, ret, param types).
fn closure_define(line: &str) -> Option<(String, String, Vec<String>, String)> {
    let rest = line.strip_prefix("define ")?;
    let at = rest.find(" @")?;
    let after = &rest[at + 2..];
    let paren = after.find('(')?;
    let param = if after[paren..].starts_with("(i64 %this_closure") {
        "%this_closure"
    } else if after[paren..].starts_with("(i64 %callee, i64 %this")
        && (after[..paren].ends_with("__eclo") || after[..paren].ends_with("__clo"))
    {
        // Public class-method function objects use the same JS body ABI,
        // but method_entries.rs spells the closure argument `callee`.
        "%callee"
    } else {
        return None;
    };
    let ret = rest[..at].rsplit(' ').next()?.to_string();
    let name = after[..paren].to_string();
    let close = after[paren..].find(')')? + paren;
    let params = after[paren + 1..close]
        .split(',')
        .map(|p| p.trim().split(' ').next().unwrap_or("").to_string())
        .collect();
    Some((name, ret, params, param.to_string()))
}

#[cfg(test)]
mod tests {
    use super::retype_closure_bodies;

    #[test]
    fn public_class_method_entries_use_the_same_pointer_abi() {
        let ir = "define double @m__eclo(i64 %callee, i64 %this) optsize {\nentry:\n  ret double 0.0\n}\n";
        let (out, retyped) = retype_closure_bodies(ir);
        assert!(out.contains("@m__eclo(ptr %callee.ptr, i64 %this)"));
        assert!(out.contains("entry:\n  %callee = ptrtoint ptr %callee.ptr to i64"));
        assert_eq!(retyped["m__eclo"].params, ["ptr", "i64"]);
    }

    #[test]
    fn closure_bodies_take_a_pointer_and_recover_the_i64() {
        let ir = "define double @perry_closure_m__0(i64 %this_closure, double %a) optsize {\n\
                  entry.0:\n  %x = add i64 %this_closure, 8\n  ret double %a\n}\n\
                  define internal double @w(i64 %this_closure) optsize personality ptr @p {\n  ret double 0.0\n}\n\
                  define double @f(double %a) {\n  ret double %a\n}\n";
        let (out, retyped) = retype_closure_bodies(ir);
        assert!(
            out.contains(
                "define double @perry_closure_m__0(ptr %this_closure.ptr, double %a) optsize {\n\
                 entry.0:\n  %this_closure = ptrtoint ptr %this_closure.ptr to i64\n  %x = add"
            ),
            "{out}"
        );
        assert!(
            out.contains("@w(ptr %this_closure.ptr) optsize personality ptr @p {\n  %this_closure = ptrtoint"),
            "{out}"
        );
        assert!(
            out.contains("define double @f(double %a) {\n  ret"),
            "{out}"
        );
        assert_eq!(retyped.len(), 2);
        let body = &retyped["perry_closure_m__0"];
        assert_eq!(
            (body.ret.as_str(), body.params.clone()),
            ("double", vec!["ptr".into(), "double".into()])
        );
    }
}
