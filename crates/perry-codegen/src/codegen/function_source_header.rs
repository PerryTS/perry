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
use perry_hir::{Expr, Function, Module as HirModule, Param};

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

/// Parameter/kind lookup for functions that are **not** `hir.functions`
/// entries. Arrow functions, function expressions and nested function
/// declarations all lower to an `Expr::Closure` nested inside an expression
/// tree, so `function_by_id` cannot see them — it searches only `hir.functions`
/// and class members. Before this existed, every one of them fell back to
/// `function () { ... }`, dropping the name *and* the parameters that the
/// documented Angular/Vue-style DI contract depends on. On `typescript@5.9.3`
/// that was ~9,600 of 9,644 functions, all interned onto one shared string.
///
/// Built from the same `closures` slice `emit_module_artifacts` already holds,
/// so this adds a map build, not a traversal.
pub(super) struct ClosureHeaders<'a> {
    by_id: HashMap<FuncId, (&'a [Param], bool)>,
}

impl<'a> ClosureHeaders<'a> {
    pub(super) fn new(closures: &'a [(FuncId, Expr)]) -> Self {
        let mut by_id = HashMap::new();
        for (func_id, expr) in closures {
            if let Expr::Closure {
                params, is_arrow, ..
            } = expr
            {
                by_id.insert(*func_id, (params.as_slice(), *is_arrow));
            }
        }
        Self { by_id }
    }

    #[cfg(test)]
    pub(super) fn empty() -> Self {
        Self {
            by_id: HashMap::new(),
        }
    }

    fn get(&self, id: FuncId) -> Option<(&'a [Param], bool)> {
        self.by_id.get(&id).copied()
    }
}

/// Original source, or the synthesized header when header mode is on.
pub(super) fn retained_function_text(
    hir: &HirModule,
    closures: &ClosureHeaders<'_>,
    func_id: FuncId,
    original: &str,
) -> String {
    if !function_source_header_mode() {
        return original.to_string();
    }
    if let Some(func) = function_by_id(hir, func_id) {
        return synthesize_function_header(&header_name(hir, func), func.params.as_slice());
    }
    if let Some((params, is_arrow)) = closures.get(func_id) {
        // An arrow has no name in source and `toString()` must not claim one,
        // nor call itself `function` - that misreports the function kind on
        // top of eliding the body.
        if is_arrow {
            return synthesize_arrow_header(params);
        }
        return synthesize_function_header(&closure_header_name(hir, func_id), params);
    }
    synthesize_function_header("", &[])
}

/// Display name for a closure that has no `hir.functions` entry — a function
/// expression or nested declaration keeps its source name here.
fn closure_header_name(hir: &HirModule, func_id: FuncId) -> String {
    match hir.closure_display_names.get(&func_id) {
        Some(display) if is_user_visible_name(display) => display.clone(),
        _ => String::new(),
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

fn synthesize_arrow_header(params: &[Param]) -> String {
    let params_src = params
        .iter()
        .filter_map(header_param)
        .collect::<Vec<_>>()
        .join(", ");
    format!("({params_src}) => {{ /* source elided */ }}")
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

    fn closure_expr(func_id: u32, params: Vec<Param>, is_arrow: bool) -> (FuncId, Expr) {
        (
            func_id,
            Expr::Closure {
                func_id,
                params,
                return_type: Type::Any,
                body: Vec::new(),
                captures: Vec::new(),
                mutable_captures: Vec::new(),
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow,
                is_async: false,
                is_generator: false,
                is_strict: false,
            },
        )
    }

    /// #10574: the regression that shipped in the first cut of header mode.
    /// A closure is not a `hir.functions` entry, so `function_by_id` misses it
    /// and the fallback produced `function () { ... }` for ~9,600 of tsc's
    /// 9,644 functions — losing the names and parameters the DI contract
    /// promises. Without `ClosureHeaders` these two assertions fail.
    #[test]
    fn closures_keep_their_names_and_parameters() {
        let hir = HirModule::new("t");
        let closures = vec![closure_expr(7, vec![param("epsilon", false)], false)];
        let headers = ClosureHeaders::new(&closures);
        let _guard = override_function_source_header_mode(true);
        assert_eq!(
            retained_function_text(&hir, &headers, 7, "function named2(epsilon) { return 1; }"),
            "function (epsilon) { /* source elided */ }"
        );
        assert!(!retained_function_text(&hir, &headers, 7, "x").contains("return"));
    }

    /// An arrow must not be reported as `function (...)`: that misstates the
    /// function *kind* on top of eliding the body.
    #[test]
    fn arrow_closures_keep_arrow_syntax() {
        let hir = HirModule::new("t");
        let closures = vec![closure_expr(
            9,
            vec![param("g", false), param("d", false)],
            true,
        )];
        let headers = ClosureHeaders::new(&closures);
        let _guard = override_function_source_header_mode(true);
        assert_eq!(
            retained_function_text(&hir, &headers, 9, "(g, d) => g + d"),
            "(g, d) => { /* source elided */ }"
        );
    }

    /// An unknown id still degrades safely rather than panicking.
    #[test]
    fn unknown_ids_fall_back_to_an_anonymous_header() {
        let hir = HirModule::new("t");
        let headers = ClosureHeaders::empty();
        let _guard = override_function_source_header_mode(true);
        assert_eq!(
            retained_function_text(&hir, &headers, 404, "whatever"),
            "function () { /* source elided */ }"
        );
    }

    /// Default mode must stay byte-identical to the original source.
    #[test]
    fn full_mode_is_byte_identical() {
        let hir = HirModule::new("t");
        let headers = ClosureHeaders::empty();
        let _guard = override_function_source_header_mode(false);
        let src = "function keepMe(a, b) { return a + b; }";
        assert_eq!(retained_function_text(&hir, &headers, 1, src), src);
    }

    #[test]
    fn compiler_params_are_omitted() {
        let text =
            synthesize_function_header("foo", &[param("__perry_cap_0", false), param("a", false)]);
        assert_eq!(text, "function foo(a) { /* source elided */ }");
    }
}
