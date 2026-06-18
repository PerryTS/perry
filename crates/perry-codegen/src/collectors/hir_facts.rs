use perry_hir::{Expr, Stmt};
use std::collections::{HashMap, HashSet};

/// Native specialization facts collected once per lowered HIR region.
///
/// A native region is a module init body, function, method, static method, or
/// closure after all HIR transforms have run and before LLVM lowering starts.
/// The graph is deliberately conservative: it only records facts consumed by
/// existing native optimizations, and every consumer must keep the normal
/// JSValue/NaN-boxed fallback at dynamic boundaries.
#[derive(Debug, Clone, Default)]
pub(crate) struct NativeRegionFactGraph {
    pub representation: RepresentationFacts,
    pub integer_range: IntegerRangeFacts,
    pub bounds: BoundsFacts,
    pub alias_noalias: AliasNoAliasFacts,
    pub escape: EscapeFacts,
    // #854: in-progress native-region fact subgraph; populated by the collector
    // (Debug field) but not yet consumed by a codegen pass.
    #[allow(dead_code)]
    pub purity: PurityFacts,
    pub platform_constants: PlatformConstantFacts,
    // #854: in-progress native-region fact subgraph; populated by the collector
    // (Debug field) but not yet consumed by a codegen pass.
    #[allow(dead_code)]
    pub shape_stability: ShapeStabilityFacts,
    pub materialization_hazards: MaterializationHazardFacts,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RepresentationFacts {
    pub integer_locals: HashSet<u32>,
    pub unsigned_i32_locals: HashSet<u32>,
    pub json_stringify_length_only_locals: HashSet<u32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct IntegerRangeFacts {
    pub index_used_locals: HashSet<u32>,
    pub strictly_i32_bounded_locals: HashSet<u32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BoundsFacts {
    pub range_seed_locals: HashSet<u32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AliasNoAliasFacts {
    pub known_noalias_buffer_locals: HashSet<u32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EscapeFacts {
    pub direct_method_new_locals: HashMap<u32, HashSet<String>>,
    pub direct_field_new_locals: HashMap<u32, HashSet<String>>,
    pub non_escaping_news: HashMap<u32, String>,
    pub non_escaping_new_used_fields: HashMap<u32, HashSet<String>>,
    pub non_escaping_arrays: HashMap<u32, u32>,
    pub non_escaping_array_used_indices: HashMap<u32, HashSet<u32>>,
    pub non_escaping_object_literals: HashMap<u32, Vec<String>>,
    pub non_escaping_object_literal_used_fields: HashMap<u32, HashSet<String>>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PurityFacts {
    // #854: in-progress purity subgraph; populated (Debug field) but no codegen
    // consumer reads it yet.
    #[allow(dead_code)]
    pub pure_helper_function_ids: HashSet<u32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PlatformConstantFacts {
    pub constants: HashMap<u32, f64>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ShapeStabilityFacts {
    // #854: in-progress shape-stability subgraph; populated (Debug field) but no
    // codegen consumer reads it yet.
    #[allow(dead_code)]
    pub scalar_replaceable_object_locals: HashSet<u32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MaterializationHazardFacts {
    pub initially_known_hazard_locals: HashSet<u32>,
}

impl NativeRegionFactGraph {
    pub(crate) fn integer_locals(&self) -> &HashSet<u32> {
        &self.representation.integer_locals
    }

    pub(crate) fn unsigned_i32_locals(&self) -> &HashSet<u32> {
        &self.representation.unsigned_i32_locals
    }

    pub(crate) fn json_stringify_length_only_locals(&self) -> &HashSet<u32> {
        &self.representation.json_stringify_length_only_locals
    }

    pub(crate) fn index_used_locals(&self) -> &HashSet<u32> {
        &self.integer_range.index_used_locals
    }

    pub(crate) fn strictly_i32_bounded_locals(&self) -> &HashSet<u32> {
        &self.integer_range.strictly_i32_bounded_locals
    }

    pub(crate) fn range_seed_locals(&self) -> &HashSet<u32> {
        &self.bounds.range_seed_locals
    }

    pub(crate) fn known_noalias_buffer_locals(&self) -> &HashSet<u32> {
        &self.alias_noalias.known_noalias_buffer_locals
    }

    pub(crate) fn compile_time_constants(&self) -> &HashMap<u32, f64> {
        &self.platform_constants.constants
    }

    pub(crate) fn non_escaping_news(&self) -> &HashMap<u32, String> {
        &self.escape.non_escaping_news
    }

    pub(crate) fn direct_method_new_locals(&self) -> &HashMap<u32, HashSet<String>> {
        &self.escape.direct_method_new_locals
    }

    pub(crate) fn direct_field_new_locals(&self) -> &HashMap<u32, HashSet<String>> {
        &self.escape.direct_field_new_locals
    }

    pub(crate) fn non_escaping_new_used_fields(&self) -> &HashMap<u32, HashSet<String>> {
        &self.escape.non_escaping_new_used_fields
    }

    pub(crate) fn non_escaping_arrays(&self) -> &HashMap<u32, u32> {
        &self.escape.non_escaping_arrays
    }

    pub(crate) fn non_escaping_array_used_indices(&self) -> &HashMap<u32, HashSet<u32>> {
        &self.escape.non_escaping_array_used_indices
    }

    pub(crate) fn non_escaping_object_literals(&self) -> &HashMap<u32, Vec<String>> {
        &self.escape.non_escaping_object_literals
    }

    pub(crate) fn non_escaping_object_literal_used_fields(&self) -> &HashMap<u32, HashSet<String>> {
        &self.escape.non_escaping_object_literal_used_fields
    }

    pub(crate) fn materialization_hazard_locals(&self) -> &HashSet<u32> {
        &self.materialization_hazards.initially_known_hazard_locals
    }
}

/// Build the full native-region fact graph in one pass boundary.
///
/// Some subgraphs still delegate to established focused collectors; this
/// function is the single contract used by codegen entry points so new native
/// consumers do not need to rediscover facts independently.
pub(crate) fn collect_native_region_fact_graph(
    stmts: &[Stmt],
    flat_const_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    classes: &HashMap<String, &perry_hir::Class>,
    compile_time_constants: &HashMap<u32, f64>,
) -> NativeRegionFactGraph {
    let integer_locals =
        super::integer_locals::collect_integer_locals(stmts, flat_const_ids, clamp_fn_ids);
    let unsigned_i32_locals = super::i32_locals::collect_unsigned_i32_locals(stmts);
    let json_stringify_length_only_locals =
        collect_json_stringify_length_only_locals(stmts, boxed_vars, module_globals);
    let index_used_locals = super::index_uses::collect_index_used_locals(stmts);
    let strictly_i32_bounded_locals = super::i32_locals::collect_strictly_i32_bounded_locals(
        stmts,
        &integer_locals,
        flat_const_ids,
        clamp_fn_ids,
    );
    let known_noalias_buffer_locals = collect_known_noalias_buffer_locals(stmts);
    let direct_method_new_locals = super::direct_method_new::collect_direct_method_new_locals(
        stmts,
        boxed_vars,
        module_globals,
        classes,
    );
    let direct_field_new_locals = super::direct_method_new::collect_direct_field_new_locals(
        stmts,
        boxed_vars,
        module_globals,
        classes,
    );
    let non_escaping_news =
        super::escape_news::collect_non_escaping_news(stmts, boxed_vars, module_globals, classes);
    let non_escaping_new_used_fields =
        super::escape_news::collect_non_escaping_new_used_fields(stmts, &non_escaping_news);
    let non_escaping_arrays =
        super::escape_arrays::collect_non_escaping_arrays(stmts, boxed_vars, module_globals);
    let non_escaping_array_used_indices =
        super::escape_arrays::collect_non_escaping_array_used_indices(stmts, &non_escaping_arrays);
    let non_escaping_object_literals = super::escape_objects::collect_non_escaping_object_literals(
        stmts,
        boxed_vars,
        module_globals,
    );
    let non_escaping_object_literal_used_fields =
        super::escape_objects::collect_non_escaping_object_literal_used_fields(
            stmts,
            &non_escaping_object_literals,
        );
    let scalar_replaceable_object_locals = non_escaping_news
        .keys()
        .chain(non_escaping_object_literals.keys())
        .copied()
        .collect();
    let graph = NativeRegionFactGraph {
        representation: RepresentationFacts {
            integer_locals: integer_locals.clone(),
            unsigned_i32_locals,
            json_stringify_length_only_locals,
        },
        integer_range: IntegerRangeFacts {
            index_used_locals,
            strictly_i32_bounded_locals,
        },
        bounds: BoundsFacts {
            range_seed_locals: integer_locals,
        },
        alias_noalias: AliasNoAliasFacts {
            known_noalias_buffer_locals,
        },
        escape: EscapeFacts {
            direct_method_new_locals,
            direct_field_new_locals,
            non_escaping_news,
            non_escaping_new_used_fields,
            non_escaping_arrays,
            non_escaping_array_used_indices,
            non_escaping_object_literals,
            non_escaping_object_literal_used_fields,
        },
        purity: PurityFacts {
            pure_helper_function_ids: clamp_fn_ids.clone(),
        },
        platform_constants: PlatformConstantFacts {
            constants: compile_time_constants.clone(),
        },
        shape_stability: ShapeStabilityFacts {
            scalar_replaceable_object_locals,
        },
        materialization_hazards: MaterializationHazardFacts::default(),
    };
    debug_assert!(graph
        .range_seed_locals()
        .is_superset(graph.integer_locals()));
    debug_assert!(graph.materialization_hazard_locals().is_empty());
    graph
}

// #854: thin wrapper over collect_native_region_fact_graph, currently only
// exercised by this module's unit tests; kept as the focused-collector entry seam.
#[allow(dead_code)]
pub(crate) fn collect_hir_facts(
    stmts: &[Stmt],
    flat_const_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
) -> NativeRegionFactGraph {
    collect_native_region_fact_graph(
        stmts,
        flat_const_ids,
        clamp_fn_ids,
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
    )
}

fn collect_known_noalias_buffer_locals(stmts: &[Stmt]) -> HashSet<u32> {
    let mut out = HashSet::new();
    let mut known_length_values = HashMap::new();
    collect_owned_buffer_lets(stmts, &mut out, &mut known_length_values);
    out
}

#[derive(Default)]
struct StringifyLengthUseState {
    candidates: HashSet<u32>,
    length_reads: HashMap<u32, usize>,
    rejected: HashSet<u32>,
}

fn collect_json_stringify_length_only_locals(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
) -> HashSet<u32> {
    let mut state = StringifyLengthUseState::default();
    collect_json_stringify_length_candidates(stmts, boxed_vars, module_globals, &mut state);
    if state.candidates.is_empty() {
        return HashSet::new();
    }
    scan_stringify_length_stmts(stmts, &mut state);
    state
        .candidates
        .into_iter()
        .filter(|id| {
            !state.rejected.contains(id) && state.length_reads.get(id).copied().unwrap_or(0) > 0
        })
        .collect()
}

fn collect_json_stringify_length_candidates(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    state: &mut StringifyLengthUseState,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                id,
                mutable,
                init: Some(init),
                ..
            } => {
                if !*mutable
                    && !boxed_vars.contains(id)
                    && !module_globals.contains_key(id)
                    && matches!(init, Expr::JsonStringifyFull(..))
                {
                    state.candidates.insert(*id);
                }
                collect_json_stringify_length_candidates_in_expr(
                    init,
                    boxed_vars,
                    module_globals,
                    state,
                );
            }
            Stmt::Let { init: None, .. } => {}
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                collect_json_stringify_length_candidates_in_expr(
                    expr,
                    boxed_vars,
                    module_globals,
                    state,
                );
            }
            Stmt::Return(None)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_json_stringify_length_candidates_in_expr(
                    condition,
                    boxed_vars,
                    module_globals,
                    state,
                );
                collect_json_stringify_length_candidates(
                    then_branch,
                    boxed_vars,
                    module_globals,
                    state,
                );
                if let Some(else_branch) = else_branch {
                    collect_json_stringify_length_candidates(
                        else_branch,
                        boxed_vars,
                        module_globals,
                        state,
                    );
                }
            }
            Stmt::While { condition, body } => {
                collect_json_stringify_length_candidates_in_expr(
                    condition,
                    boxed_vars,
                    module_globals,
                    state,
                );
                collect_json_stringify_length_candidates(body, boxed_vars, module_globals, state);
            }
            Stmt::DoWhile { body, condition } => {
                collect_json_stringify_length_candidates(body, boxed_vars, module_globals, state);
                collect_json_stringify_length_candidates_in_expr(
                    condition,
                    boxed_vars,
                    module_globals,
                    state,
                );
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    collect_json_stringify_length_candidates(
                        std::slice::from_ref(init.as_ref()),
                        boxed_vars,
                        module_globals,
                        state,
                    );
                }
                if let Some(condition) = condition {
                    collect_json_stringify_length_candidates_in_expr(
                        condition,
                        boxed_vars,
                        module_globals,
                        state,
                    );
                }
                if let Some(update) = update {
                    collect_json_stringify_length_candidates_in_expr(
                        update,
                        boxed_vars,
                        module_globals,
                        state,
                    );
                }
                collect_json_stringify_length_candidates(body, boxed_vars, module_globals, state);
            }
            Stmt::Labeled { body, .. } => {
                collect_json_stringify_length_candidates(
                    std::slice::from_ref(body.as_ref()),
                    boxed_vars,
                    module_globals,
                    state,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_json_stringify_length_candidates(body, boxed_vars, module_globals, state);
                if let Some(catch) = catch {
                    collect_json_stringify_length_candidates(
                        &catch.body,
                        boxed_vars,
                        module_globals,
                        state,
                    );
                }
                if let Some(finally) = finally {
                    collect_json_stringify_length_candidates(
                        finally,
                        boxed_vars,
                        module_globals,
                        state,
                    );
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                collect_json_stringify_length_candidates_in_expr(
                    discriminant,
                    boxed_vars,
                    module_globals,
                    state,
                );
                for case in cases {
                    if let Some(test) = &case.test {
                        collect_json_stringify_length_candidates_in_expr(
                            test,
                            boxed_vars,
                            module_globals,
                            state,
                        );
                    }
                    collect_json_stringify_length_candidates(
                        &case.body,
                        boxed_vars,
                        module_globals,
                        state,
                    );
                }
            }
        }
    }
}

fn collect_json_stringify_length_candidates_in_expr(
    expr: &Expr,
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    state: &mut StringifyLengthUseState,
) {
    if matches!(expr, Expr::Closure { .. }) {
        return;
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        collect_json_stringify_length_candidates_in_expr(child, boxed_vars, module_globals, state)
    });
}

fn scan_stringify_length_stmts(stmts: &[Stmt], state: &mut StringifyLengthUseState) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(init) = init {
                    scan_stringify_length_expr(init, state);
                }
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                scan_stringify_length_expr(expr, state);
            }
            Stmt::Return(None)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                scan_stringify_length_expr(condition, state);
                scan_stringify_length_stmts(then_branch, state);
                if let Some(else_branch) = else_branch {
                    scan_stringify_length_stmts(else_branch, state);
                }
            }
            Stmt::While { condition, body } => {
                scan_stringify_length_expr(condition, state);
                scan_stringify_length_stmts(body, state);
            }
            Stmt::DoWhile { body, condition } => {
                scan_stringify_length_stmts(body, state);
                scan_stringify_length_expr(condition, state);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    scan_stringify_length_stmts(std::slice::from_ref(init.as_ref()), state);
                }
                if let Some(condition) = condition {
                    scan_stringify_length_expr(condition, state);
                }
                if let Some(update) = update {
                    scan_stringify_length_expr(update, state);
                }
                scan_stringify_length_stmts(body, state);
            }
            Stmt::Labeled { body, .. } => {
                scan_stringify_length_stmts(std::slice::from_ref(body.as_ref()), state);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                scan_stringify_length_stmts(body, state);
                if let Some(catch) = catch {
                    scan_stringify_length_stmts(&catch.body, state);
                }
                if let Some(finally) = finally {
                    scan_stringify_length_stmts(finally, state);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                scan_stringify_length_expr(discriminant, state);
                for case in cases {
                    if let Some(test) = &case.test {
                        scan_stringify_length_expr(test, state);
                    }
                    scan_stringify_length_stmts(&case.body, state);
                }
            }
            Stmt::PreallocateBoxes(ids) => {
                for id in ids {
                    reject_stringify_length_local(*id, state);
                }
            }
        }
    }
}

fn scan_stringify_length_expr(expr: &Expr, state: &mut StringifyLengthUseState) {
    match expr {
        Expr::PropertyGet { object, property } if property == "length" => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if state.candidates.contains(id) {
                    *state.length_reads.entry(*id).or_insert(0) += 1;
                    return;
                }
            }
        }
        Expr::LocalGet(id) => {
            reject_stringify_length_local(*id, state);
        }
        Expr::LocalSet(id, value) => {
            reject_stringify_length_local(*id, state);
            scan_stringify_length_expr(value, state);
            return;
        }
        Expr::Update { id, .. } => {
            reject_stringify_length_local(*id, state);
            return;
        }
        Expr::Delete(target) => {
            reject_stringify_length_refs_in_expr(target, state);
            return;
        }
        Expr::Closure {
            captures,
            mutable_captures,
            ..
        } => {
            for id in captures.iter().chain(mutable_captures.iter()) {
                reject_stringify_length_local(*id, state);
            }
            return;
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        scan_stringify_length_expr(child, state)
    });
}

fn reject_stringify_length_refs_in_expr(expr: &Expr, state: &mut StringifyLengthUseState) {
    match expr {
        Expr::LocalGet(id) | Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
            reject_stringify_length_local(*id, state);
        }
        Expr::Closure {
            captures,
            mutable_captures,
            ..
        } => {
            for id in captures.iter().chain(mutable_captures.iter()) {
                reject_stringify_length_local(*id, state);
            }
            return;
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        reject_stringify_length_refs_in_expr(child, state)
    });
}

fn reject_stringify_length_local(id: u32, state: &mut StringifyLengthUseState) {
    if state.candidates.contains(&id) {
        state.rejected.insert(id);
    }
}

fn collect_owned_buffer_lets_child_scope(
    stmts: &[Stmt],
    out: &mut HashSet<u32>,
    known_length_values: &HashMap<u32, i64>,
) {
    let mut child_length_values = known_length_values.clone();
    collect_owned_buffer_lets(stmts, out, &mut child_length_values);
}

fn collect_owned_buffer_lets(
    stmts: &[Stmt],
    out: &mut HashSet<u32>,
    known_length_values: &mut HashMap<u32, i64>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                id,
                mutable,
                init: Some(init),
                ..
            } => {
                if !*mutable && is_owned_u8_buffer_alloc(init, known_length_values) {
                    out.insert(*id);
                }
                if !*mutable {
                    if let Some(length) = fresh_uint8array_length_value(init, known_length_values) {
                        known_length_values.insert(*id, length);
                    } else {
                        known_length_values.remove(id);
                    }
                } else {
                    known_length_values.remove(id);
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_owned_buffer_lets_child_scope(then_branch, out, known_length_values);
                if let Some(else_branch) = else_branch {
                    collect_owned_buffer_lets_child_scope(else_branch, out, known_length_values);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_owned_buffer_lets_child_scope(body, out, known_length_values);
            }
            Stmt::For { init, body, .. } => {
                let mut loop_length_values = known_length_values.clone();
                if let Some(init) = init {
                    collect_owned_buffer_lets(
                        std::slice::from_ref(init.as_ref()),
                        out,
                        &mut loop_length_values,
                    );
                }
                collect_owned_buffer_lets(body, out, &mut loop_length_values);
            }
            Stmt::Labeled { body, .. } => {
                collect_owned_buffer_lets_child_scope(
                    std::slice::from_ref(body.as_ref()),
                    out,
                    known_length_values,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_owned_buffer_lets_child_scope(body, out, known_length_values);
                if let Some(catch) = catch {
                    collect_owned_buffer_lets_child_scope(&catch.body, out, known_length_values);
                }
                if let Some(finally) = finally {
                    collect_owned_buffer_lets_child_scope(finally, out, known_length_values);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_owned_buffer_lets_child_scope(&case.body, out, known_length_values);
                }
            }
            Stmt::Let { init: None, .. }
            | Stmt::Expr(_)
            | Stmt::Return(_)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::Throw(_)
            | Stmt::PreallocateBoxes(_) => {}
        }
    }
}

fn is_owned_u8_buffer_alloc(expr: &Expr, known_length_values: &HashMap<u32, i64>) -> bool {
    match expr {
        Expr::BufferAlloc { .. } | Expr::BufferAllocUnsafe(_) => true,
        Expr::Uint8ArrayNew(None) => true,
        Expr::Uint8ArrayNew(Some(size)) => {
            fresh_uint8array_length_value(size, known_length_values).is_some()
        }
        Expr::TypedArrayNew { arg: None, .. } => true,
        Expr::TypedArrayNew {
            arg: Some(size), ..
        } => fresh_uint8array_length_value(size, known_length_values).is_some(),
        Expr::NativeMethodCall {
            module,
            method,
            object: None,
            ..
        } if module == "buffer" && method == "copyBytesFrom" => true,
        Expr::NativeArenaView { .. } => true,
        _ => false,
    }
}

fn fresh_uint8array_length_value(
    expr: &Expr,
    known_length_values: &HashMap<u32, i64>,
) -> Option<i64> {
    match expr {
        Expr::Integer(n) => valid_uint8array_length(*n),
        Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => valid_uint8array_length(*n as i64),
        Expr::LocalGet(id) => known_length_values.get(id).copied(),
        Expr::Binary { op, left, right } => {
            let lhs = fresh_uint8array_length_value(left, known_length_values)?;
            let rhs = fresh_uint8array_length_value(right, known_length_values)?;
            let value = match op {
                perry_hir::BinaryOp::Add => lhs.checked_add(rhs)?,
                perry_hir::BinaryOp::Sub => lhs.checked_sub(rhs)?,
                perry_hir::BinaryOp::Mul => lhs.checked_mul(rhs)?,
                perry_hir::BinaryOp::Div if rhs != 0 && lhs % rhs == 0 => lhs / rhs,
                _ => return None,
            };
            valid_uint8array_length(value)
        }
        _ => None,
    }
}

fn valid_uint8array_length(value: i64) -> Option<i64> {
    (value >= 0 && value < i32::MAX as i64).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::{BinaryOp, Class, ClassField, CompareOp, Function, UpdateOp};
    use perry_types::Type;

    fn const_let(id: u32, init: Expr) -> Stmt {
        Stmt::Let {
            id,
            name: format!("v{}", id),
            ty: Type::Named("Uint8Array".into()),
            mutable: false,
            init: Some(init),
        }
    }

    fn const_number_let(id: u32, init: Expr) -> Stmt {
        Stmt::Let {
            id,
            name: format!("v{}", id),
            ty: Type::Number,
            mutable: false,
            init: Some(init),
        }
    }

    fn known_ids(stmts: Vec<Stmt>) -> HashSet<u32> {
        collect_known_noalias_buffer_locals(&stmts)
    }

    fn mutable_number_let(id: u32, init: Expr) -> Stmt {
        Stmt::Let {
            id,
            name: format!("v{}", id),
            ty: Type::Number,
            mutable: true,
            init: Some(init),
        }
    }

    fn ushr0(left: Expr) -> Expr {
        Expr::Binary {
            op: BinaryOp::UShr,
            left: Box::new(left),
            right: Box::new(Expr::Integer(0)),
        }
    }

    fn binary(op: BinaryOp, left: Expr, right: Expr) -> Expr {
        Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn stringify_full(value: Expr) -> Expr {
        Expr::JsonStringifyFull(
            Box::new(value),
            Box::new(Expr::Undefined),
            Box::new(Expr::Undefined),
        )
    }

    fn empty_function(id: u32, name: &str, return_type: Type, body: Vec<Stmt>) -> Function {
        Function {
            id,
            name: name.to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type,
            body,
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    fn counter_class() -> Class {
        Class {
            id: 1,
            name: "Counter".to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            fields: vec![ClassField {
                name: "value".to_string(),
                key_expr: None,
                ty: Type::Number,
                init: None,
                is_private: false,
                is_readonly: false,
                decorators: Vec::new(),
            }],
            constructor: Some(empty_function(
                2,
                "Counter_constructor",
                Type::Void,
                vec![Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::This),
                    property: "value".to_string(),
                    value: Box::new(Expr::Integer(0)),
                })],
            )),
            methods: vec![
                empty_function(
                    3,
                    "increment",
                    Type::Void,
                    vec![Stmt::Expr(Expr::PropertySet {
                        object: Box::new(Expr::This),
                        property: "value".to_string(),
                        value: Box::new(Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(Expr::PropertyGet {
                                object: Box::new(Expr::This),
                                property: "value".to_string(),
                            }),
                            right: Box::new(Expr::Integer(1)),
                        }),
                    })],
                ),
                empty_function(
                    4,
                    "get",
                    Type::Number,
                    vec![Stmt::Return(Some(Expr::PropertyGet {
                        object: Box::new(Expr::This),
                        property: "value".to_string(),
                    }))],
                ),
            ],
            getters: Vec::new(),
            setters: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
        }
    }

    #[test]
    fn uint8array_literal_lengths_are_known_noalias_sources() {
        let ids = known_ids(vec![
            const_let(1, Expr::Uint8ArrayNew(None)),
            const_let(2, Expr::Uint8ArrayNew(Some(Box::new(Expr::Integer(8))))),
            const_let(3, Expr::Uint8ArrayNew(Some(Box::new(Expr::Number(16.0))))),
        ]);

        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
    }

    #[test]
    fn uint8array_const_local_lengths_are_known_noalias_sources() {
        let ids = known_ids(vec![
            const_number_let(10, Expr::Integer(8)),
            const_let(1, Expr::Uint8ArrayNew(Some(Box::new(Expr::LocalGet(10))))),
            const_number_let(11, Expr::Number(16.0)),
            const_number_let(12, Expr::LocalGet(11)),
            const_let(2, Expr::Uint8ArrayNew(Some(Box::new(Expr::LocalGet(12))))),
        ]);

        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
    }

    #[test]
    fn uint8array_const_arithmetic_lengths_are_known_noalias_sources() {
        let ids = known_ids(vec![
            const_number_let(10, Expr::Integer(100)),
            const_let(
                1,
                Expr::Uint8ArrayNew(Some(Box::new(binary(
                    BinaryOp::Mul,
                    Expr::LocalGet(10),
                    Expr::LocalGet(10),
                )))),
            ),
            const_number_let(11, Expr::Integer(i32::MAX as i64 - 1)),
            const_let(
                2,
                Expr::Uint8ArrayNew(Some(Box::new(binary(
                    BinaryOp::Mul,
                    Expr::LocalGet(11),
                    Expr::Integer(2),
                )))),
            ),
        ]);

        assert!(ids.contains(&1));
        assert!(!ids.contains(&2));
    }

    #[test]
    fn uint8array_non_literal_or_alias_possible_sources_are_not_noalias() {
        let ids = known_ids(vec![
            const_let(1, Expr::Uint8ArrayNew(Some(Box::new(Expr::LocalGet(99))))),
            const_let(2, Expr::Uint8ArrayNew(Some(Box::new(Expr::Integer(-1))))),
            const_let(3, Expr::Uint8ArrayNew(Some(Box::new(Expr::Number(3.5))))),
            const_let(4, Expr::Uint8ArrayNew(Some(Box::new(Expr::Number(-1.0))))),
            const_let(
                5,
                Expr::Uint8ArrayNew(Some(Box::new(Expr::Number(i32::MAX as f64)))),
            ),
            mutable_number_let(6, Expr::Integer(8)),
            const_let(7, Expr::Uint8ArrayNew(Some(Box::new(Expr::LocalGet(6))))),
        ]);

        assert!(ids.is_empty(), "unexpected noalias ids: {ids:?}");
    }

    #[test]
    fn mutable_ushr_zero_recurrence_is_unsigned_i32_not_signed_integer() {
        let facts = collect_hir_facts(
            &[
                const_let(1, ushr0(Expr::Integer(0x9E3779B9))),
                mutable_number_let(2, ushr0(Expr::LocalGet(1))),
                Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(ushr0(Expr::Binary {
                        op: BinaryOp::BitXor,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Integer(0x1234)),
                    })),
                )),
            ],
            &HashSet::new(),
            &HashSet::new(),
        );

        assert!(facts.unsigned_i32_locals().contains(&2));
        assert!(!facts.integer_locals().contains(&2));
    }

    #[test]
    fn signed_write_disqualifies_unsigned_i32_local() {
        let facts = collect_hir_facts(
            &[
                mutable_number_let(2, ushr0(Expr::Integer(0x9E3779B9))),
                Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(Expr::Binary {
                        op: BinaryOp::BitOr,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Integer(0)),
                    }),
                )),
            ],
            &HashSet::new(),
            &HashSet::new(),
        );

        assert!(!facts.unsigned_i32_locals().contains(&2));
    }

    #[test]
    fn native_fact_graph_collects_platform_purity_and_noalias_subgraphs() {
        let mut constants = HashMap::new();
        constants.insert(90, 1.0);
        let mut pure_helpers = HashSet::new();
        pure_helpers.insert(7);

        let graph = collect_native_region_fact_graph(
            &[const_let(
                1,
                Expr::Uint8ArrayNew(Some(Box::new(Expr::Integer(8)))),
            )],
            &HashSet::new(),
            &pure_helpers,
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &constants,
        );

        assert!(graph.known_noalias_buffer_locals().contains(&1));
        assert_eq!(graph.compile_time_constants().get(&90), Some(&1.0));
        assert!(graph.purity.pure_helper_function_ids.contains(&7));
    }

    #[test]
    fn native_fact_graph_collects_range_and_shape_escape_facts() {
        let stmts = vec![
            mutable_number_let(1, Expr::Integer(0)),
            Stmt::Expr(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(2)),
                index: Box::new(Expr::LocalGet(1)),
            }),
            Stmt::Let {
                id: 3,
                name: "o".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Object(vec![("x".to_string(), Expr::Integer(1))])),
            },
        ];

        let graph = collect_native_region_fact_graph(
            &stmts,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        assert!(graph.integer_locals().contains(&1));
        assert!(graph.index_used_locals().contains(&1));
        assert!(graph.non_escaping_object_literals().contains_key(&3));
        assert!(graph
            .shape_stability
            .scalar_replaceable_object_locals
            .contains(&3));
    }

    #[test]
    fn native_fact_graph_collects_json_stringify_length_only_locals() {
        let stmts = vec![
            Stmt::Let {
                id: 2,
                name: "json".to_string(),
                ty: Type::String,
                mutable: false,
                init: Some(stringify_full(Expr::LocalGet(1))),
            },
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(2)),
                property: "length".to_string(),
            })),
        ];

        let graph = collect_native_region_fact_graph(
            &stmts,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        assert!(graph.json_stringify_length_only_locals().contains(&2));
    }

    #[test]
    fn native_fact_graph_keeps_json_stringify_length_facts_scoped_to_closure_body() {
        let closure_body = vec![
            Stmt::Let {
                id: 2,
                name: "json".to_string(),
                ty: Type::String,
                mutable: false,
                init: Some(stringify_full(Expr::Integer(1))),
            },
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(2)),
                property: "length".to_string(),
            })),
        ];
        let stmts = vec![Stmt::Let {
            id: 9,
            name: "callback".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Closure {
                func_id: 101,
                params: Vec::new(),
                return_type: Type::Number,
                body: closure_body.clone(),
                captures: Vec::new(),
                mutable_captures: Vec::new(),
                captures_this: false,
                enclosing_class: None,
                is_async: false,
                is_generator: false,
                is_strict: false,
            }),
        }];

        let parent_graph = collect_native_region_fact_graph(
            &stmts,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let closure_graph = collect_native_region_fact_graph(
            &closure_body,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        assert!(!parent_graph
            .json_stringify_length_only_locals()
            .contains(&2));
        assert!(closure_graph
            .json_stringify_length_only_locals()
            .contains(&2));
    }

    #[test]
    fn native_fact_graph_rejects_json_stringify_locals_with_value_uses() {
        let stmts = vec![
            Stmt::Let {
                id: 2,
                name: "json".to_string(),
                ty: Type::String,
                mutable: false,
                init: Some(stringify_full(Expr::LocalGet(1))),
            },
            Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(2)),
                    property: "length".to_string(),
                }),
                right: Box::new(Expr::LocalGet(2)),
            })),
        ];

        let graph = collect_native_region_fact_graph(
            &stmts,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        assert!(!graph.json_stringify_length_only_locals().contains(&2));
    }

    #[test]
    fn direct_field_new_local_survives_inlined_loop_field_update() {
        let counter = counter_class();
        let classes = HashMap::from([("Counter".to_string(), &counter)]);
        let stmts = vec![
            Stmt::Let {
                id: 1,
                name: "counter".to_string(),
                ty: Type::Named("Counter".to_string()),
                mutable: false,
                init: Some(Expr::New {
                    class_name: "Counter".to_string(),
                    args: Vec::new(),
                    type_args: Vec::new(),
                }),
            },
            Stmt::For {
                init: Some(Box::new(mutable_number_let(7, Expr::Integer(0)))),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(7)),
                    right: Box::new(Expr::Integer(10)),
                }),
                update: Some(Expr::Update {
                    id: 7,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::LocalGet(1)),
                    property: "value".to_string(),
                    value: Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(1)),
                            property: "value".to_string(),
                        }),
                        right: Box::new(Expr::Integer(1)),
                    }),
                })],
            },
        ];

        let graph = collect_native_region_fact_graph(
            &stmts,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashMap::new(),
            &classes,
            &HashMap::new(),
        );

        assert!(
            graph
                .direct_field_new_locals()
                .get(&1)
                .is_some_and(|fields| fields.contains("value")),
            "direct field facts: {:?}",
            graph.direct_field_new_locals()
        );
    }

    #[test]
    fn direct_field_new_local_rejects_deleted_field() {
        let counter = counter_class();
        let classes = HashMap::from([("Counter".to_string(), &counter)]);
        let stmts = vec![
            Stmt::Let {
                id: 1,
                name: "counter".to_string(),
                ty: Type::Named("Counter".to_string()),
                mutable: false,
                init: Some(Expr::New {
                    class_name: "Counter".to_string(),
                    args: Vec::new(),
                    type_args: Vec::new(),
                }),
            },
            Stmt::Expr(Expr::Delete(Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(1)),
                property: "value".to_string(),
            }))),
            Stmt::Expr(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(1)),
                property: "value".to_string(),
            }),
        ];

        let graph = collect_native_region_fact_graph(
            &stmts,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashMap::new(),
            &classes,
            &HashMap::new(),
        );

        assert!(
            !graph.direct_field_new_locals().contains_key(&1),
            "direct field facts should reject deleted candidates: {:?}",
            graph.direct_field_new_locals()
        );
    }
}
