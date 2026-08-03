//! Interprocedural integer-range summaries for numeric function parameters
//! (#7286, lever (c)).
//!
//! `numeric_index_has_integer_array_index_proof` admits an array index only
//! when its range is provably inside `[0, i32::MAX]`. Ranges compose through
//! `+`/`*`, so **one unbounded leaf poisons the whole index expression** — and
//! a numeric *parameter* is always unbounded, because it arrives as a bare
//! NaN-boxed `double` with no fact attached. In `16_matrix_multiply`,
//! `matmul(a, b, c, size: number)` is exactly that: `size` has no range, so
//! `a[i * size + k]`, `b[k * size + j]` and `c[i * size + j]` all fall to the
//! opaque `js_array_get_index_or_string` /
//! `js_typed_feedback_array_set_index_or_string` helpers — 67.1M calls at
//! 256³.
//!
//! This pre-pass closes that hole for the narrow case where the whole call
//! graph of a parameter is visible and every caller passes an integer
//! constant. It is a **meet over all call sites**, and *any* unresolved
//! reference to the function poisons it entirely.
//!
//! # Proof obligations
//!
//! A `(function, parameter)` pair earns a range only when all of these hold:
//!
//! 1. **Every call is visible.** The function is not exported (so no other
//!    module can call it), is never reflected onto the global object, and its
//!    `FuncId` appears *only* as the callee of a direct `Expr::Call`. Any
//!    other occurrence — a bare `Expr::FuncRef` used as a value, an
//!    `Expr::Closure` with the same id, a `CallSpread` whose arity is unknown
//!    — poisons the function.
//! 2. **Arity is fixed.** No rest parameter, no default value, no `arguments`
//!    object, and every call site passes exactly `params.len()` arguments.
//!    Otherwise the slot can hold `undefined`, whose "range" is nothing.
//! 3. **The parameter is never rebound.** It is never a `LocalSet` / `Update`
//!    target and never re-declared by a body `Stmt::Let` (`var` hoisting
//!    reuses a parameter's id for `function f(x) { var x = … }`), at any depth
//!    including inside closure bodies.
//! 4. **Every argument is an integer constant** — a literal, or a read of a
//!    top-level `const` already folded into `compile_time_constants`.
//!
//! Under-approximation is free: a missing summary just keeps today's
//! runtime-key helper, which is the correct answer, not a failure.

use std::collections::{HashMap, HashSet};

use perry_hir::{Expr, Function, Module, Param, Stmt};

use crate::expr::IntRange;

/// LocalId of a parameter → its meet-over-call-sites integer range.
pub(crate) type ParamIntRanges = HashMap<u32, IntRange>;

#[derive(Default)]
struct Scan {
    /// FuncIds that can be entered by something other than a visible direct
    /// call, so their argument set is not knowable.
    poisoned: HashSet<u32>,
    /// FuncId → per-argument-position constant, one entry per direct call
    /// site. `None` where the argument is not an integer constant.
    call_sites: HashMap<u32, Vec<Vec<Option<i64>>>>,
    /// Locals written (`LocalSet` / `GlobalSet` / `Update`) at any depth.
    writes: HashSet<u32>,
    /// Locals re-declared by a `Stmt::Let` at any depth.
    rebinds: HashSet<u32>,
}

fn constant_arg(expr: &Expr, module_constants: &HashMap<u32, f64>) -> Option<i64> {
    let value = match expr {
        Expr::Integer(n) => return Some(*n),
        Expr::Number(n) => *n,
        Expr::LocalGet(id) => *module_constants.get(id)?,
        _ => return None,
    };
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }
    let min = i64::MIN as f64;
    let max = i64::MAX as f64;
    (value >= min && value <= max).then(|| value as i64)
}

fn visit_expr(expr: &Expr, module_constants: &HashMap<u32, f64>, scan: &mut Scan) {
    match expr {
        Expr::Call { callee, args, .. } => {
            if let Expr::FuncRef(fid) = callee.as_ref() {
                let site: Vec<Option<i64>> = args
                    .iter()
                    .map(|arg| constant_arg(arg, module_constants))
                    .collect();
                scan.call_sites.entry(*fid).or_default().push(site);
                for arg in args {
                    visit_expr(arg, module_constants, scan);
                }
                return;
            }
        }
        // A `FuncRef` anywhere other than a direct callee is the function
        // escaping as a value: it can be stored, passed, `.call`ed, or
        // returned, and then invoked with arguments this pass cannot see.
        Expr::FuncRef(fid) => {
            scan.poisoned.insert(*fid);
            return;
        }
        Expr::Closure { func_id, .. } => {
            scan.poisoned.insert(*func_id);
        }
        Expr::LocalSet(id, _) | Expr::GlobalSet(id, _) | Expr::Update { id, .. } => {
            scan.writes.insert(*id);
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        visit_expr(child, module_constants, scan)
    });
}

fn visit_stmts(stmts: &[Stmt], module_constants: &HashMap<u32, f64>, scan: &mut Scan) {
    for stmt in stmts {
        visit_stmt(stmt, module_constants, scan);
    }
}

fn visit_stmt(stmt: &Stmt, module_constants: &HashMap<u32, f64>, scan: &mut Scan) {
    match stmt {
        Stmt::Let { id, init, .. } => {
            scan.rebinds.insert(*id);
            if let Some(init) = init {
                visit_expr(init, module_constants, scan);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => {
            visit_expr(e, module_constants, scan)
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr(condition, module_constants, scan);
            visit_stmts(then_branch, module_constants, scan);
            if let Some(body) = else_branch {
                visit_stmts(body, module_constants, scan);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            visit_expr(condition, module_constants, scan);
            visit_stmts(body, module_constants, scan);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                visit_stmt(init, module_constants, scan);
            }
            if let Some(condition) = condition {
                visit_expr(condition, module_constants, scan);
            }
            if let Some(update) = update {
                visit_expr(update, module_constants, scan);
            }
            visit_stmts(body, module_constants, scan);
        }
        Stmt::Labeled { body, .. } => visit_stmt(body, module_constants, scan),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            visit_stmts(body, module_constants, scan);
            if let Some(catch) = catch {
                if let Some((id, _)) = &catch.param {
                    scan.rebinds.insert(*id);
                }
                visit_stmts(&catch.body, module_constants, scan);
            }
            if let Some(body) = finally {
                visit_stmts(body, module_constants, scan);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr(discriminant, module_constants, scan);
            for case in cases {
                if let Some(test) = &case.test {
                    visit_expr(test, module_constants, scan);
                }
                visit_stmts(&case.body, module_constants, scan);
            }
        }
    }
}

fn visit_params(params: &[Param], module_constants: &HashMap<u32, f64>, scan: &mut Scan) {
    for param in params {
        if let Some(default) = &param.default {
            visit_expr(default, module_constants, scan);
        }
    }
}

/// Callee-side admission: fixed arity, no `arguments` object, and a shape
/// whose entry is only ever the direct-call path.
fn function_shape_is_summarizable(f: &Function) -> bool {
    !f.is_exported
        && !f.is_async
        && !f.is_generator
        && !f.params.is_empty()
        && f.params
            .iter()
            .all(|p| !p.is_rest && p.default.is_none() && p.arguments_object.is_none())
}

/// Meet the argument constants seen at every call site of every summarizable
/// function into a per-parameter [`IntRange`].
pub(crate) fn collect_param_int_ranges(
    hir: &Module,
    module_constants: &HashMap<u32, f64>,
) -> ParamIntRanges {
    let mut scan = Scan::default();
    visit_stmts(&hir.init, module_constants, &mut scan);
    for f in &hir.functions {
        visit_params(&f.params, module_constants, &mut scan);
        visit_stmts(&f.body, module_constants, &mut scan);
    }
    for c in &hir.classes {
        for f in c
            .constructor
            .iter()
            .chain(c.methods.iter())
            .chain(c.static_methods.iter())
            .chain(c.getters.iter().map(|(_, g)| g))
            .chain(c.setters.iter().map(|(_, s)| s))
            .chain(c.computed_members.iter().map(|cm| &cm.function))
        {
            visit_params(&f.params, module_constants, &mut scan);
            visit_stmts(&f.body, module_constants, &mut scan);
        }
        for cm in &c.computed_members {
            visit_expr(&cm.key_expr, module_constants, &mut scan);
        }
        for field in c.fields.iter().chain(c.static_fields.iter()) {
            if let Some(key) = &field.key_expr {
                visit_expr(key, module_constants, &mut scan);
            }
            if let Some(init) = &field.init {
                visit_expr(init, module_constants, &mut scan);
            }
        }
    }

    // A Script's top-level `function` declarations become own properties of
    // the global object when the program mentions `globalThis`, at which point
    // `globalThis.f(…)` is a call site this pass cannot enumerate.
    if hir.references_global_this {
        for (_, fid) in &hir.script_global_functions {
            scan.poisoned.insert(*fid);
        }
    }

    let mut ranges = ParamIntRanges::new();
    for f in &hir.functions {
        if scan.poisoned.contains(&f.id) || !function_shape_is_summarizable(f) {
            continue;
        }
        let Some(sites) = scan.call_sites.get(&f.id) else {
            continue;
        };
        if sites.is_empty() || sites.iter().any(|site| site.len() != f.params.len()) {
            continue;
        }
        for (idx, param) in f.params.iter().enumerate() {
            if scan.writes.contains(&param.id) || scan.rebinds.contains(&param.id) {
                continue;
            }
            let mut min = i64::MAX;
            let mut max = i64::MIN;
            let mut proven = true;
            for site in sites {
                match site[idx] {
                    Some(value) => {
                        min = min.min(value);
                        max = max.max(value);
                    }
                    None => {
                        proven = false;
                        break;
                    }
                }
            }
            if proven {
                ranges.insert(param.id, IntRange { min, max });
            }
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;

    const SIZE_CONST: u32 = 1;
    const PARAM_A: u32 = 10;
    const PARAM_SIZE: u32 = 11;
    const MATMUL: u32 = 5;

    fn param(id: u32, name: &str) -> Param {
        Param {
            id,
            name: name.to_string(),
            ty: Type::Number,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }
    }

    fn matmul_fn(body: Vec<Stmt>) -> Function {
        Function {
            id: MATMUL,
            name: "matmul".to_string(),
            type_params: Vec::new(),
            params: vec![param(PARAM_A, "a"), param(PARAM_SIZE, "size")],
            return_type: Type::Void,
            body,
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    fn call(args: Vec<Expr>) -> Stmt {
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(MATMUL)),
            args,
            type_args: Vec::new(),
            byte_offset: 0,
        })
    }

    fn module(init: Vec<Stmt>, functions: Vec<Function>) -> Module {
        let mut hir = Module::new("test");
        hir.functions = functions;
        hir.init = init;
        hir
    }

    fn constants() -> HashMap<u32, f64> {
        HashMap::from([(SIZE_CONST, 256.0)])
    }

    /// Lever (c): `matmul(a, SIZE)` with a module-level `const SIZE = 256`
    /// pins `size` to exactly 256, which is what lets `i * size + k` prove
    /// `[0, 65535]`.
    #[test]
    fn single_constant_call_site_pins_the_parameter() {
        let hir = module(
            vec![call(vec![Expr::Integer(0), Expr::LocalGet(SIZE_CONST)])],
            vec![matmul_fn(Vec::new())],
        );
        let ranges = collect_param_int_ranges(&hir, &constants());
        assert_eq!(ranges.get(&PARAM_SIZE), Some(&IntRange::exact(256)));
        assert_eq!(ranges.get(&PARAM_A), Some(&IntRange::exact(0)));
    }

    /// Two call sites meet into the covering interval.
    #[test]
    fn multiple_call_sites_meet() {
        let hir = module(
            vec![
                call(vec![Expr::Integer(0), Expr::Integer(4)]),
                call(vec![Expr::Integer(0), Expr::Integer(64)]),
            ],
            vec![matmul_fn(Vec::new())],
        );
        let ranges = collect_param_int_ranges(&hir, &constants());
        assert_eq!(ranges.get(&PARAM_SIZE), Some(&IntRange { min: 4, max: 64 }));
    }

    /// One non-constant argument leaves the parameter unproven.
    #[test]
    fn one_unresolved_call_site_poisons_the_parameter() {
        let hir = module(
            vec![
                call(vec![Expr::Integer(0), Expr::Integer(4)]),
                call(vec![Expr::Integer(0), Expr::LocalGet(99)]),
            ],
            vec![matmul_fn(Vec::new())],
        );
        assert!(collect_param_int_ranges(&hir, &constants())
            .get(&PARAM_SIZE)
            .is_none());
    }

    /// The function escaping as a value means unseen call sites.
    #[test]
    fn func_ref_used_as_a_value_poisons_the_function() {
        let mut init = vec![call(vec![Expr::Integer(0), Expr::Integer(4)])];
        init.push(Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(77)),
            args: vec![Expr::FuncRef(MATMUL)],
            type_args: Vec::new(),
            byte_offset: 0,
        }));
        let hir = module(init, vec![matmul_fn(Vec::new())]);
        assert!(collect_param_int_ranges(&hir, &constants()).is_empty());
    }

    /// An exported function can be called from another module, which this
    /// per-module pass never sees.
    #[test]
    fn exported_function_is_not_summarized() {
        let mut f = matmul_fn(Vec::new());
        f.is_exported = true;
        let hir = module(vec![call(vec![Expr::Integer(0), Expr::Integer(4)])], vec![f]);
        assert!(collect_param_int_ranges(&hir, &constants()).is_empty());
    }

    /// A rest parameter makes the arity — and therefore the slot's value —
    /// unknowable at the callee.
    #[test]
    fn rest_parameter_is_not_summarized() {
        let mut f = matmul_fn(Vec::new());
        f.params[1].is_rest = true;
        let hir = module(vec![call(vec![Expr::Integer(0), Expr::Integer(4)])], vec![f]);
        assert!(collect_param_int_ranges(&hir, &constants()).is_empty());
    }

    /// A call site with fewer arguments leaves the slot `undefined`.
    #[test]
    fn arity_mismatch_is_not_summarized() {
        let hir = module(
            vec![call(vec![Expr::Integer(0)])],
            vec![matmul_fn(Vec::new())],
        );
        assert!(collect_param_int_ranges(&hir, &constants()).is_empty());
    }

    /// A parameter the body reassigns no longer holds the caller's value.
    #[test]
    fn reassigned_parameter_is_not_summarized() {
        let body = vec![Stmt::Expr(Expr::LocalSet(
            PARAM_SIZE,
            Box::new(Expr::Integer(-1)),
        ))];
        let hir = module(
            vec![call(vec![Expr::Integer(0), Expr::Integer(4)])],
            vec![matmul_fn(body)],
        );
        let ranges = collect_param_int_ranges(&hir, &constants());
        assert!(ranges.get(&PARAM_SIZE).is_none());
        // The untouched sibling parameter keeps its summary.
        assert_eq!(ranges.get(&PARAM_A), Some(&IntRange::exact(0)));
    }

    /// `function f(x) { var x = … }` rebinds the parameter's slot id.
    #[test]
    fn rebound_parameter_is_not_summarized() {
        let body = vec![Stmt::Let {
            id: PARAM_SIZE,
            name: "size".to_string(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Integer(-1)),
        }];
        let hir = module(
            vec![call(vec![Expr::Integer(0), Expr::Integer(4)])],
            vec![matmul_fn(body)],
        );
        assert!(collect_param_int_ranges(&hir, &constants())
            .get(&PARAM_SIZE)
            .is_none());
    }

    /// A never-called function gets no summary (an empty meet is not `[⊥, ⊤]`).
    #[test]
    fn uncalled_function_is_not_summarized() {
        let hir = module(Vec::new(), vec![matmul_fn(Vec::new())]);
        assert!(collect_param_int_ranges(&hir, &constants()).is_empty());
    }

    /// Reflected onto `globalThis`, the function is reachable by name.
    #[test]
    fn global_this_reflected_function_is_not_summarized() {
        let mut hir = module(
            vec![call(vec![Expr::Integer(0), Expr::Integer(4)])],
            vec![matmul_fn(Vec::new())],
        );
        hir.references_global_this = true;
        hir.script_global_functions = vec![("matmul".to_string(), MATMUL)];
        assert!(collect_param_int_ranges(&hir, &constants()).is_empty());
    }

    /// A fractional argument is not an integer range.
    #[test]
    fn fractional_argument_is_not_summarized() {
        let hir = module(
            vec![call(vec![Expr::Integer(0), Expr::Number(1.5)])],
            vec![matmul_fn(Vec::new())],
        );
        assert!(collect_param_int_ranges(&hir, &constants())
            .get(&PARAM_SIZE)
            .is_none());
    }
}
