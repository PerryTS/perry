//! #10574 Part 2: retain a synthesized `Function.prototype.toString` header
//! instead of the function body.
//!
//! Default remains the interned original source (Part 1). `header` mode stores
//! `function <name>(<params>) { /* source elided */ }`, which is enough for
//! name extraction, Angular/Vue-style parameter-name DI, and
//! `toString().includes("[native code]")` probes, and drops the remaining
//! ~6 MB of unique function source on a tsc-sized bundle.
//!
//! Opt in with `--function-source=header` or `PERRY_FUNCTION_SOURCE=header`.
//! Full source is the default so `fn.toString()` stays spec-identical and
//! first-party worker serialization (`perry-threads`) keeps working.

use std::cell::Cell;
use std::collections::HashMap;

use perry_hir::types::FuncId;
use perry_hir::{Function, Module as HirModule, Param};

thread_local! {
    static HEADER_MODE_OVERRIDE: Cell<Option<bool>> = const { Cell::new(None) };
}

/// True when codegen should emit the synthesized header instead of the body.
pub(super) fn function_source_header_mode() -> bool {
    if let Some(overridden) = HEADER_MODE_OVERRIDE.with(Cell::get) {
        return overridden;
    }
    matches!(
        std::env::var("PERRY_FUNCTION_SOURCE").as_deref(),
        Ok("header") | Ok("elide")
    )
}

/// RAII override for unit tests. Restores the previous override on drop so
/// parallel tests on this thread cannot leak the mode into a later case.
#[cfg(test)]
pub(super) struct FunctionSourceHeaderGuard(Option<bool>);

#[cfg(test)]
impl Drop for FunctionSourceHeaderGuard {
    fn drop(&mut self) {
        HEADER_MODE_OVERRIDE.with(|cell| cell.set(self.0));
    }
}

#[cfg(test)]
pub(super) fn override_function_source_header_mode(on: bool) -> FunctionSourceHeaderGuard {
    FunctionSourceHeaderGuard(HEADER_MODE_OVERRIDE.with(|cell| cell.replace(Some(on))))
}

/// Original source, or the synthesized header when header mode is on.
pub(super) fn retained_function_text(hir: &HirModule, func_id: FuncId, original: &str) -> String {
    if !function_source_header_mode() {
        return original.to_string();
    }
    match function_by_id(hir, func_id) {
        Some(func) => synthesize_function_header(&header_name(hir, func), func.params.as_slice()),
        None => synthesize_function_header("", &[]),
    }
}

/// Header-mode class source map. `None` on the default path so the caller
/// can pass `hir.class_source_text` without cloning it.
pub(super) fn elide_class_sources(hir: &HirModule) -> Option<HashMap<u32, String>> {
    if !function_source_header_mode() {
        return None;
    }
    Some(
        hir.class_source_text
            .keys()
            .map(|&cid| (cid, synthesize_class_header(hir, cid)))
            .collect(),
    )
}

fn header_name(hir: &HirModule, func: &Function) -> String {
    if let Some(display) = hir.closure_display_names.get(&func.id) {
        if is_user_visible_name(display) {
            return display.clone();
        }
    }
    if is_user_visible_name(&func.name) {
        func.name.clone()
    } else {
        String::new()
    }
}

fn is_user_visible_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with("__perry")
        && !name.starts_with("perry_")
        && !name.starts_with("__Anon")
        && !name.starts_with("__anon")
}

fn synthesize_function_header(name: &str, params: &[Param]) -> String {
    let params_src = params
        .iter()
        .filter_map(header_param)
        .collect::<Vec<_>>()
        .join(", ");
    if name.is_empty() {
        format!("function ({params_src}) {{ /* source elided */ }}")
    } else {
        format!("function {name}({params_src}) {{ /* source elided */ }}")
    }
}

fn header_param(param: &Param) -> Option<String> {
    if param.arguments_object.is_some() {
        return None;
    }
    if !is_user_visible_name(&param.name) {
        return None;
    }
    if param.is_rest {
        Some(format!("...{}", param.name))
    } else {
        Some(param.name.clone())
    }
}

fn synthesize_class_header(hir: &HirModule, cid: u32) -> String {
    let name = hir
        .class_display_names
        .get(&cid)
        .cloned()
        .or_else(|| {
            hir.classes
                .iter()
                .find(|class| class.id == cid)
                .map(|class| class.name.clone())
        })
        .filter(|name| is_user_visible_name(name));
    match name {
        Some(name) => format!("class {name} {{ /* source elided */ }}"),
        None => "class { /* source elided */ }".to_string(),
    }
}

fn function_by_id(hir: &HirModule, id: FuncId) -> Option<&Function> {
    if let Some(func) = hir.functions.iter().find(|func| func.id == id) {
        return Some(func);
    }
    for class in &hir.classes {
        if let Some(ctor) = &class.constructor {
            if ctor.id == id {
                return Some(ctor);
            }
        }
        for func in class
            .methods
            .iter()
            .chain(class.static_methods.iter())
            .chain(class.getters.iter().map(|(_, func)| func))
            .chain(class.setters.iter().map(|(_, func)| func))
        {
            if func.id == id {
                return Some(func);
            }
        }
        if let Some(member) = class
            .computed_members
            .iter()
            .find(|member| member.function.id == id)
        {
            return Some(&member.function);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;
    use perry_hir::Param;

    fn param(name: &str, rest: bool) -> Param {
        Param {
            id: 1,
            name: name.to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: rest,
            arguments_object: None,
        }
    }

    #[test]
    fn named_function_keeps_parameter_names_and_drops_the_body() {
        let text = synthesize_function_header("foo", &[param("a", false), param("b", false)]);
        assert_eq!(text, "function foo(a, b) { /* source elided */ }");
        assert!(text.starts_with("function foo("));
        assert!(!text.contains("return"));
    }

    #[test]
    fn anonymous_and_rest_params_round_trip_the_di_header() {
        assert_eq!(
            synthesize_function_header("", &[param("x", false), param("rest", true)]),
            "function (x, ...rest) { /* source elided */ }"
        );
    }

    #[test]
    fn compiler_params_are_omitted() {
        let text =
            synthesize_function_header("foo", &[param("__perry_cap_0", false), param("a", false)]);
        assert_eq!(text, "function foo(a) { /* source elided */ }");
    }
}
