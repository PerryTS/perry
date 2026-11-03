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
        // Another module's body, reached by a guarded direct call: its
        // definition is retyped there, so route this module's calls through
        // the same adapter (`abi_adapt` rewrites the declaration too).
        if let Some((name, ret, mut params)) = imported_body_declare(line) {
            params[0] = "ptr".to_string();
            retyped.insert(name, Retyped { ret, params });
        }
        out.push_str(line);
    }
    (out, retyped)
}

/// Give every indirect call of a JS body the body's pointer-width ABI.
///
/// [`retype_closure_bodies`] retypes the definitions; the calls built through
/// `expr::body_call` reach a body by code pointer and still spell the callee
/// as `i64`. A wasm `call_indirect` must match the callee's signature
/// exactly, so such a call traps (`indirect call type mismatch`). Rewrite each
/// indirect `double`-returning call whose first two arguments are the
/// `(i64 callee, i64 this)` pair to pass the callee as `ptr`; a native
/// argument-list body (`JsNativeArgsBody`, `(callee, this, *const f64,
/// usize)`) also takes its length as an `i32`.
pub(super) fn retype_indirect_body_calls(ir: &str) -> String {
    let mut out = String::with_capacity(ir.len() + 1024);
    let mut next = 0usize;
    for line in ir.split_inclusive('\n') {
        match adapt_indirect_call(line, &mut next) {
            Some((prelude, call)) => {
                out.push_str(&prelude);
                out.push_str(&call);
            }
            None => out.push_str(line),
        }
    }
    out
}

fn adapt_indirect_call(line: &str, next: &mut usize) -> Option<(String, String)> {
    let call_at = [" call double %", " invoke double %"]
        .iter()
        .find_map(|kw| line.find(kw).map(|at| at + kw.len() - 1))?;
    let open = call_at + line[call_at..].find('(')?;
    let close = open + line[open..].find(')')?;
    let mut args: Vec<&str> = line[open + 1..close].split(", ").collect();
    let value = |arg: &str, ty: &str| arg.strip_prefix(ty)?.strip_prefix(' ').map(str::to_string);
    let callee = value(args.first()?, "i64")?;
    value(args.get(1)?, "i64")?;
    let native_args = args.len() == 4 && args[2].starts_with("ptr ") && args[3].starts_with("i64 ");
    if args.len() > 2 && !native_args && !args[2..].iter().all(|a| a.starts_with("double ")) {
        return None;
    }
    let mut prelude = String::new();
    let id = *next;
    *next += 1;
    let callee_ptr = format!("%body.callee.{id}");
    prelude.push_str(&format!("  {callee_ptr} = inttoptr i64 {callee} to ptr\n"));
    let first = format!("ptr {callee_ptr}");
    args[0] = &first;
    let argc;
    if native_args {
        let len = value(args[3], "i64")?;
        let len_i32 = format!("%body.argc.{id}");
        prelude.push_str(&format!("  {len_i32} = trunc i64 {len} to i32\n"));
        argc = format!("i32 {len_i32}");
        args[3] = &argc;
    }
    let rewritten = format!("{}({}{}", &line[..open], args.join(", "), &line[close..]);
    Some((prelude, rewritten))
}

/// `define [linkage …] <ret> @name(i64 %this_closure, <ty> %x, …) … {`
/// → (name, ret, param types).
/// `declare <ret> @__perry_wrap_perry_fn_…(i64, i64, …)`: a JS body wrapper
/// another module defines (every such definition takes `i64 %this_closure`).
fn imported_body_declare(line: &str) -> Option<(String, String, Vec<String>)> {
    let rest = line.strip_prefix("declare ")?;
    let at = rest.find(" @__perry_wrap_perry_fn_")?;
    let after = &rest[at + 2..];
    let paren = after.find('(')?;
    let close = after[paren..].find(')')? + paren;
    let params: Vec<String> = after[paren + 1..close]
        .split(',')
        .map(|p| p.trim().to_string())
        .collect();
    if params.len() < 2 || params[0] != "i64" || params[1] != "i64" {
        return None;
    }
    let ret = rest[..at].rsplit(' ').next()?.to_string();
    Some((after[..paren].to_string(), ret, params))
}

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
    use super::{retype_closure_bodies, retype_indirect_body_calls};

    #[test]
    fn indirect_body_calls_pass_the_callee_as_a_pointer() {
        let ir = "  %r = call double %fp(i64 %c, i64 %t, double %a)\n\
                  %r2 = invoke double %fp(i64 %c, i64 %t) to label %k unwind label %u\n\
                  %r3 = call double %fp(i64 %c, i64 %t, ptr %args, i64 %n) \"gc-leaf-function\"\n\
                  %r4 = call double @direct(i64 %c, i64 %t, double %a)\n\
                  %r5 = call double %fp(i64 %x, double %a)\n";
        let out = retype_indirect_body_calls(ir);
        assert!(out.contains("  %body.callee.0 = inttoptr i64 %c to ptr\n  %r = call double %fp(ptr %body.callee.0, i64 %t, double %a)\n"), "{out}");
        assert!(
            out.contains("%r2 = invoke double %fp(ptr %body.callee.1, i64 %t) to label"),
            "{out}"
        );
        assert!(out.contains("%body.argc.2 = trunc i64 %n to i32"), "{out}");
        assert!(out.contains("call double %fp(ptr %body.callee.2, i64 %t, ptr %args, i32 %body.argc.2) \"gc-leaf-function\""), "{out}");
        assert!(
            out.contains("%r4 = call double @direct(i64 %c, i64 %t, double %a)\n"),
            "{out}"
        );
        assert!(
            out.contains("%r5 = call double %fp(i64 %x, double %a)\n"),
            "{out}"
        );
    }

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
