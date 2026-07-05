use perry_hir::{ArrayElement, Expr, Stmt};
use std::collections::{HashMap, HashSet};

/// Native specialization facts collected once per lowered HIR region.
///
/// A native region is a module init body, function, method, static method, or
/// closure after all HIR transforms have run and before LLVM lowering starts.
/// The graph is deliberately conservative: it only records facts consumed by
/// existing native optimizations, and every consumer must keep the normal
/// JSValue/NaN-boxed fallback at dynamic boundaries.
#[derive(Debug, Clone, Default)]
pub(crate) struct TypeFacts {
    pub representation: RepresentationFacts,
    pub arrays: ArrayFacts,
    pub effect: EffectFacts,
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

pub(crate) type NativeRegionFactGraph = TypeFacts;

#[derive(Debug, Clone, Default)]
pub(crate) struct RepresentationFacts {
    pub integer_locals: HashSet<u32>,
    pub unsigned_i32_locals: HashSet<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArrayKindFact {
    PackedI32,
    PackedU32,
    PackedF64,
    PackedValue,
    HoleyValue,
    Unknown,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ArrayFacts {
    pub local_kinds: HashMap<u32, ArrayKindFact>,
    pub length_stable_locals: HashSet<u32>,
    pub noalias_locals: HashSet<u32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EffectFacts {
    pub unknown_call_escape: bool,
    pub async_microtask_escape: bool,
    pub array_length_mutation_locals: HashSet<u32>,
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

#[allow(dead_code)]
impl TypeFacts {
    pub(crate) fn integer_locals(&self) -> &HashSet<u32> {
        &self.representation.integer_locals
    }

    pub(crate) fn unsigned_i32_locals(&self) -> &HashSet<u32> {
        &self.representation.unsigned_i32_locals
    }

    pub(crate) fn array_kind(&self, local_id: u32) -> ArrayKindFact {
        self.arrays
            .local_kinds
            .get(&local_id)
            .copied()
            .unwrap_or(ArrayKindFact::Unknown)
    }

    pub(crate) fn proves_packed_f64_array(&self, local_id: u32) -> bool {
        self.array_kind(local_id) == ArrayKindFact::PackedF64
            && self.proves_noalias_array(local_id)
            && self.proves_array_length_stable(local_id)
            && !self.has_materialization_hazard(local_id)
    }

    /// #6011: packed-f64 eligibility for arrays *written* by the range-guarded
    /// versioned loop. Unlike [`Self::proves_packed_f64_array`] this does NOT
    /// require length stability — an `arr[i] = …` in the loop body marks the
    /// local as length-mutating even though the range guard proves every such
    /// store is in-bounds (and therefore length-preserving) at loop entry.
    /// Kind + noalias + no-hazard is what the guarded store path needs.
    pub(crate) fn packed_f64_eligible_for_guarded_store(&self, local_id: u32) -> bool {
        self.array_kind(local_id) == ArrayKindFact::PackedF64
            && self.proves_noalias_array(local_id)
            && !self.has_materialization_hazard(local_id)
    }

    pub(crate) fn proves_packed_i32_array(&self, local_id: u32) -> bool {
        self.array_kind(local_id) == ArrayKindFact::PackedI32
            && self.proves_noalias_array(local_id)
            && self.proves_array_length_stable(local_id)
            && !self.has_materialization_hazard(local_id)
    }

    pub(crate) fn proves_packed_u32_array(&self, local_id: u32) -> bool {
        self.array_kind(local_id) == ArrayKindFact::PackedU32
            && self.proves_noalias_array(local_id)
            && self.proves_array_length_stable(local_id)
            && !self.has_materialization_hazard(local_id)
    }

    pub(crate) fn proves_array_length_stable(&self, local_id: u32) -> bool {
        self.arrays.length_stable_locals.contains(&local_id)
    }

    pub(crate) fn proves_noalias_array(&self, local_id: u32) -> bool {
        self.arrays.noalias_locals.contains(&local_id)
    }

    pub(crate) fn array_length_mutation_locals(&self) -> &HashSet<u32> {
        &self.effect.array_length_mutation_locals
    }

    pub(crate) fn has_unknown_call_escape(&self) -> bool {
        self.effect.unknown_call_escape
    }

    pub(crate) fn has_async_microtask_escape(&self) -> bool {
        self.effect.async_microtask_escape
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

    pub(crate) fn proves_i32_lowering(&self, local_id: u32) -> bool {
        self.representation.integer_locals.contains(&local_id)
            || self
                .integer_range
                .strictly_i32_bounded_locals
                .contains(&local_id)
    }

    pub(crate) fn proves_unsigned_i32_lowering(&self, local_id: u32) -> bool {
        self.representation.unsigned_i32_locals.contains(&local_id)
    }

    pub(crate) fn proves_bounds_range_seed(&self, local_id: u32) -> bool {
        self.bounds.range_seed_locals.contains(&local_id)
    }

    pub(crate) fn proves_noalias_buffer(&self, local_id: u32) -> bool {
        self.alias_noalias
            .known_noalias_buffer_locals
            .contains(&local_id)
    }

    pub(crate) fn proves_pure_helper(&self, function_id: u32) -> bool {
        self.purity.pure_helper_function_ids.contains(&function_id)
    }

    pub(crate) fn platform_constant(&self, local_id: u32) -> Option<f64> {
        self.platform_constants.constants.get(&local_id).copied()
    }

    pub(crate) fn scalar_replaceable_object_locals(&self) -> &HashSet<u32> {
        &self.shape_stability.scalar_replaceable_object_locals
    }

    pub(crate) fn proves_scalar_replacement(&self, local_id: u32) -> bool {
        self.shape_stability
            .scalar_replaceable_object_locals
            .contains(&local_id)
            || self.escape.non_escaping_arrays.contains_key(&local_id)
    }

    pub(crate) fn has_materialization_hazard(&self, local_id: u32) -> bool {
        self.materialization_hazards
            .initially_known_hazard_locals
            .contains(&local_id)
    }
}

/// Build the full native-region fact graph in one pass boundary.
///
/// Some subgraphs still delegate to established focused collectors; this
/// function is the single contract used by codegen entry points so new native
/// consumers do not need to rediscover facts independently.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_type_facts(
    stmts: &[Stmt],
    params: &[perry_hir::Param],
    flat_const_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
    arg_dependent_clamp_fn_ids: &HashSet<u32>,
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    classes: &HashMap<String, &perry_hir::Class>,
    compile_time_constants: &HashMap<u32, f64>,
) -> TypeFacts {
    let integer_locals = super::integer_locals::collect_integer_locals(
        stmts,
        flat_const_ids,
        clamp_fn_ids,
        arg_dependent_clamp_fn_ids,
    );
    let unsigned_i32_locals = super::i32_locals::collect_unsigned_i32_locals(stmts);
    let (array_facts, effect_facts, materialization_hazards) = collect_array_facts(stmts, params);
    let index_used_locals = super::index_uses::collect_index_used_locals(stmts);
    let strictly_i32_bounded_locals = super::i32_locals::collect_strictly_i32_bounded_locals(
        stmts,
        &integer_locals,
        flat_const_ids,
        clamp_fn_ids,
    );
    let known_noalias_buffer_locals = collect_known_noalias_buffer_locals(stmts);
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
    let graph = TypeFacts {
        representation: RepresentationFacts {
            integer_locals: integer_locals.clone(),
            unsigned_i32_locals,
        },
        arrays: array_facts,
        effect: effect_facts,
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
        materialization_hazards,
    };
    debug_assert!(graph
        .range_seed_locals()
        .is_superset(graph.integer_locals()));
    debug_assert!(graph.arrays.length_stable_locals.iter().all(|id| {
        !graph.has_materialization_hazard(*id) && !graph.array_length_mutation_locals().contains(id)
    }));
    graph
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_native_region_fact_graph(
    stmts: &[Stmt],
    params: &[perry_hir::Param],
    flat_const_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
    arg_dependent_clamp_fn_ids: &HashSet<u32>,
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    classes: &HashMap<String, &perry_hir::Class>,
    compile_time_constants: &HashMap<u32, f64>,
) -> NativeRegionFactGraph {
    collect_type_facts(
        stmts,
        params,
        flat_const_ids,
        clamp_fn_ids,
        arg_dependent_clamp_fn_ids,
        boxed_vars,
        module_globals,
        classes,
        compile_time_constants,
    )
}

// #854: thin wrapper over collect_type_facts, currently only exercised by this
// module's unit tests; kept as the focused-collector entry point.
#[allow(dead_code)]
pub(crate) fn collect_hir_facts(
    stmts: &[Stmt],
    flat_const_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
) -> TypeFacts {
    collect_type_facts(
        stmts,
        &[],
        flat_const_ids,
        clamp_fn_ids,
        &HashSet::new(),
        &HashSet::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
    )
}

fn collect_known_noalias_buffer_locals(stmts: &[Stmt]) -> HashSet<u32> {
    let mut out = HashSet::new();
    let mut known_length_locals = HashSet::new();
    collect_owned_buffer_lets(stmts, &mut out, &mut known_length_locals);
    out
}

fn collect_owned_buffer_lets_child_scope(
    stmts: &[Stmt],
    out: &mut HashSet<u32>,
    known_length_locals: &HashSet<u32>,
) {
    let mut child_length_locals = known_length_locals.clone();
    collect_owned_buffer_lets(stmts, out, &mut child_length_locals);
}

fn collect_owned_buffer_lets(
    stmts: &[Stmt],
    out: &mut HashSet<u32>,
    known_length_locals: &mut HashSet<u32>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                id,
                mutable,
                init: Some(init),
                ..
            } => {
                if !*mutable && is_owned_u8_buffer_alloc(init, known_length_locals) {
                    out.insert(*id);
                }
                if !*mutable && is_fresh_uint8array_length_expr(init, known_length_locals) {
                    known_length_locals.insert(*id);
                } else {
                    known_length_locals.remove(id);
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_owned_buffer_lets_child_scope(then_branch, out, known_length_locals);
                if let Some(else_branch) = else_branch {
                    collect_owned_buffer_lets_child_scope(else_branch, out, known_length_locals);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_owned_buffer_lets_child_scope(body, out, known_length_locals);
            }
            Stmt::For { init, body, .. } => {
                let mut loop_length_locals = known_length_locals.clone();
                if let Some(init) = init {
                    collect_owned_buffer_lets(
                        std::slice::from_ref(init.as_ref()),
                        out,
                        &mut loop_length_locals,
                    );
                }
                collect_owned_buffer_lets(body, out, &mut loop_length_locals);
            }
            Stmt::Labeled { body, .. } => {
                collect_owned_buffer_lets_child_scope(
                    std::slice::from_ref(body.as_ref()),
                    out,
                    known_length_locals,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_owned_buffer_lets_child_scope(body, out, known_length_locals);
                if let Some(catch) = catch {
                    collect_owned_buffer_lets_child_scope(&catch.body, out, known_length_locals);
                }
                if let Some(finally) = finally {
                    collect_owned_buffer_lets_child_scope(finally, out, known_length_locals);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_owned_buffer_lets_child_scope(&case.body, out, known_length_locals);
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

fn is_owned_u8_buffer_alloc(expr: &Expr, known_length_locals: &HashSet<u32>) -> bool {
    match expr {
        Expr::BufferAlloc { .. } | Expr::BufferAllocUnsafe(_) => true,
        Expr::Uint8ArrayNew(None) => true,
        Expr::Uint8ArrayNew(Some(size)) => {
            is_fresh_uint8array_length_expr(size, known_length_locals)
        }
        Expr::TypedArrayNew { arg: None, .. } => true,
        Expr::TypedArrayNew {
            arg: Some(size), ..
        } => is_fresh_uint8array_length_expr(size, known_length_locals),
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

fn is_fresh_uint8array_length_expr(expr: &Expr, known_length_locals: &HashSet<u32>) -> bool {
    match expr {
        Expr::LocalGet(id) => known_length_locals.contains(id),
        _ => is_fresh_uint8array_length_literal(expr),
    }
}

fn is_fresh_uint8array_length_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Integer(n) => *n >= 0 && *n < i32::MAX as i64,
        Expr::Number(n) => n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n < i32::MAX as f64,
        _ => false,
    }
}

fn collect_array_facts(
    stmts: &[Stmt],
    params: &[perry_hir::Param],
) -> (ArrayFacts, EffectFacts, MaterializationHazardFacts) {
    let mut collector = ArrayFactCollector::default();
    collector.seed_params(params);
    collector.collect_stmts(stmts);
    collector.finish()
}

#[derive(Default)]
struct ArrayFactCollector {
    local_kinds: HashMap<u32, ArrayKindFact>,
    aliases: HashMap<u32, u32>,
    aliased_locals: HashSet<u32>,
    length_mutation_locals: HashSet<u32>,
    materialization_hazard_locals: HashSet<u32>,
    /// #6011: param ids seeded purely from a declared `Packed*` array type.
    /// A param can receive ANY array at runtime, so these seeds are only
    /// versioning hints for the runtime-guard-validated packed loop matcher;
    /// body-observed mutations still downgrade them like any tracked local.
    param_seeded_locals: HashSet<u32>,
    unknown_call_escape: bool,
    async_microtask_escape: bool,
}

impl ArrayFactCollector {
    /// #6011: seed `local_kinds` for function params whose *declared* type is
    /// a packed numeric array (e.g. `prices: number[]`). Only the numeric
    /// `Packed*` kinds are seeded — a `PackedValue`/`HoleyValue` param fact
    /// enables no consumer and would only widen the conservative-downgrade
    /// surface. Seeding happens before the body walk so every mutation the
    /// walk observes (push, aliasing, length writes, …) downgrades the param
    /// exactly like a `Stmt::Let`-declared array.
    fn seed_params(&mut self, params: &[perry_hir::Param]) {
        for param in params {
            if param.is_rest {
                continue;
            }
            let kind = array_kind_from_declared_type(&param.ty);
            if matches!(
                kind,
                ArrayKindFact::PackedI32 | ArrayKindFact::PackedU32 | ArrayKindFact::PackedF64
            ) {
                self.local_kinds.insert(param.id, kind);
                self.param_seeded_locals.insert(param.id);
            }
        }
    }

    fn collect_stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.collect_stmt(stmt);
        }
    }

    fn collect_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { id, ty, init, .. } => {
                let declared_kind = array_kind_from_declared_type(ty);
                if declared_kind != ArrayKindFact::Unknown {
                    let combined_kind = init
                        .as_ref()
                        .map(|expr| array_kind_from_declared_initializer(declared_kind, expr))
                        .unwrap_or(ArrayKindFact::Unknown);
                    self.local_kinds.insert(*id, combined_kind);
                }
                if let Some(init) = init {
                    self.collect_expr(init);
                    self.record_local_alias_write(*id, init);
                } else {
                    self.aliases.remove(id);
                }
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                self.collect_expr(expr);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.collect_expr(condition);
                self.collect_stmts(then_branch);
                if let Some(else_branch) = else_branch {
                    self.collect_stmts(else_branch);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.collect_expr(condition);
                self.collect_stmts(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.collect_stmt(init.as_ref());
                }
                if let Some(condition) = condition {
                    self.collect_expr(condition);
                }
                if let Some(update) = update {
                    self.collect_expr(update);
                }
                self.collect_stmts(body);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.collect_stmts(body);
                if let Some(catch) = catch {
                    self.collect_stmts(&catch.body);
                }
                if let Some(finally) = finally {
                    self.collect_stmts(finally);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.collect_expr(discriminant);
                for case in cases {
                    if let Some(test) = &case.test {
                        self.collect_expr(test);
                    }
                    self.collect_stmts(&case.body);
                }
            }
            Stmt::Labeled { body, .. } => self.collect_stmt(body.as_ref()),
            Stmt::Return(None)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_) => {}
        }
    }

    fn collect_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::ArrayPush { array_id, value } => {
                let value_kind = if expr_is_i32_shaped(value) {
                    ArrayKindFact::PackedI32
                } else if expr_is_numeric_shaped(value) {
                    ArrayKindFact::PackedF64
                } else {
                    ArrayKindFact::PackedValue
                };
                self.mark_array_length_mutation(*array_id, value_kind);
                self.collect_expr(value);
            }
            Expr::ArrayPushSpread { array_id, source } => {
                self.mark_array_length_mutation(*array_id, ArrayKindFact::Unknown);
                self.collect_expr(source);
            }
            Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
                self.mark_array_length_mutation(*id, ArrayKindFact::HoleyValue);
            }
            Expr::ArrayUnshift { array_id, value } => {
                self.mark_array_length_mutation(*array_id, ArrayKindFact::Unknown);
                self.collect_expr(value);
            }
            Expr::ArraySplice {
                array_id,
                start,
                delete_count,
                items,
            } => {
                self.mark_array_length_mutation(*array_id, ArrayKindFact::Unknown);
                self.collect_expr(start);
                if let Some(delete_count) = delete_count {
                    self.collect_expr(delete_count);
                }
                for item in items {
                    self.collect_expr(item);
                }
            }
            Expr::Array(elements) => {
                for element in elements {
                    self.mark_array_identity_exposure(element);
                    self.collect_expr(element);
                }
            }
            Expr::ArraySpread(elements) => {
                for element in elements {
                    match element {
                        ArrayElement::Expr(expr) => {
                            self.mark_array_identity_exposure(expr);
                            self.collect_expr(expr);
                        }
                        ArrayElement::Spread(expr) => {
                            self.collect_expr(expr);
                        }
                        ArrayElement::Hole => {}
                    }
                }
            }
            Expr::Object(fields) => {
                for (_, value) in fields {
                    self.mark_array_identity_exposure(value);
                    self.collect_expr(value);
                }
            }
            Expr::ObjectSpread { parts } => {
                for (key, value) in parts {
                    if key.is_some() {
                        self.mark_array_identity_exposure(value);
                    }
                    self.collect_expr(value);
                }
            }
            Expr::ArrayCopyWithin {
                array_id,
                target,
                start,
                end,
            } => {
                self.mark_array_materialization_hazard(*array_id);
                self.update_array_kind_for_local(*array_id, ArrayKindFact::Unknown);
                self.collect_expr(target);
                self.collect_expr(start);
                if let Some(end) = end {
                    self.collect_expr(end);
                }
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    let value_kind = if expr_is_i32_shaped(value) {
                        ArrayKindFact::PackedI32
                    } else if expr_is_numeric_shaped(value) {
                        ArrayKindFact::PackedF64
                    } else {
                        ArrayKindFact::PackedValue
                    };
                    self.mark_array_length_mutation(*id, value_kind);
                }
                self.collect_expr(object);
                self.collect_expr(index);
                self.collect_expr(value);
            }
            Expr::IndexUpdate { object, index, .. } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    self.mark_array_length_mutation(*id, ArrayKindFact::Unknown);
                }
                self.collect_expr(object);
                self.collect_expr(index);
            }
            Expr::LocalSet(id, value) => {
                if self.tracked_array_root(*id).is_some() {
                    self.mark_array_materialization_hazard(*id);
                    self.update_array_kind_for_local(*id, ArrayKindFact::Unknown);
                }
                self.collect_expr(value);
                self.record_local_alias_write(*id, value);
            }
            Expr::PropertySet {
                object,
                property,
                value,
            } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if property == "length" {
                        self.mark_array_length_mutation(*id, ArrayKindFact::Unknown);
                    } else {
                        self.mark_array_materialization_hazard(*id);
                    }
                }
                self.collect_expr(object);
                self.collect_expr(value);
            }
            Expr::PropertyUpdate { object, .. } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    self.mark_array_materialization_hazard(*id);
                }
                self.collect_expr(object);
            }
            Expr::ObjectFreeze(target)
            | Expr::ObjectSeal(target)
            | Expr::ObjectPreventExtensions(target) => {
                self.mark_array_target_materialization_hazard(target);
                self.collect_expr(target);
            }
            Expr::ObjectDefineProperty(target, key, descriptor)
            | Expr::ReflectDefineProperty {
                target,
                key,
                descriptor,
            } => {
                self.mark_array_target_materialization_hazard(target);
                self.collect_expr(target);
                self.collect_expr(key);
                self.collect_expr(descriptor);
            }
            Expr::ObjectDefineProperties(target, descriptors) => {
                self.mark_array_target_materialization_hazard(target);
                self.collect_expr(target);
                self.collect_expr(descriptors);
            }
            Expr::ObjectSetPrototypeOf(target, proto)
            | Expr::ReflectSetPrototypeOf { target, proto } => {
                self.mark_array_target_materialization_hazard(target);
                self.collect_expr(target);
                self.collect_expr(proto);
            }
            Expr::ObjectAssign { target, sources } => {
                self.mark_array_target_materialization_hazard(target);
                self.collect_expr(target);
                for source in sources {
                    self.collect_expr(source);
                }
            }
            Expr::ArraySort { array, comparator } => {
                self.mark_array_target_materialization_hazard(array);
                self.mark_unknown_call_escape();
                self.collect_expr(array);
                self.collect_expr(comparator);
            }
            Expr::ArrayForEach { array, callback }
            | Expr::ArrayMap { array, callback }
            | Expr::ArrayFilter { array, callback }
            | Expr::ArrayFind { array, callback }
            | Expr::ArrayFindIndex { array, callback }
            | Expr::ArrayFindLast { array, callback }
            | Expr::ArrayFindLastIndex { array, callback }
            | Expr::ArraySome { array, callback }
            | Expr::ArrayEvery { array, callback }
            | Expr::ArrayFlatMap { array, callback }
            | Expr::ArrayReduce {
                array,
                callback,
                initial: _,
            }
            | Expr::ArrayReduceRight {
                array,
                callback,
                initial: _,
            } => {
                self.mark_unknown_call_escape();
                self.collect_expr(array);
                self.collect_expr(callback);
                perry_hir::walker::walk_expr_children(expr, &mut |child| {
                    if !std::ptr::eq(child, array.as_ref())
                        && !std::ptr::eq(child, callback.as_ref())
                    {
                        self.collect_expr(child);
                    }
                });
            }
            Expr::ArrayReverseValue { receiver }
            | Expr::ArrayCopyWithinValue {
                receiver,
                target: _,
                start: _,
                end: _,
            } => {
                self.mark_array_target_materialization_hazard(receiver);
                perry_hir::walker::walk_expr_children(expr, &mut |child| {
                    self.collect_expr(child);
                });
            }
            Expr::Call { callee, args, .. } => {
                self.mark_unknown_call_escape();
                self.collect_expr(callee);
                for arg in args {
                    self.collect_expr(arg);
                }
            }
            Expr::CallSpread { callee, args, .. } => {
                self.mark_unknown_call_escape();
                self.collect_expr(callee);
                for arg in args {
                    let inner = match arg {
                        perry_hir::CallArg::Expr(expr) | perry_hir::CallArg::Spread(expr) => expr,
                    };
                    self.collect_expr(inner);
                }
            }
            Expr::NativeMethodCall { object, args, .. } => {
                self.mark_unknown_call_escape();
                if let Some(object) = object {
                    self.collect_expr(object);
                }
                for arg in args {
                    self.collect_expr(arg);
                }
            }
            Expr::NewDynamic { callee, args, .. } => {
                self.mark_unknown_call_escape();
                self.collect_expr(callee);
                for arg in args {
                    self.collect_expr(arg);
                }
            }
            Expr::NewDynamicSpread { callee, args, .. } => {
                self.mark_unknown_call_escape();
                self.collect_expr(callee);
                for arg in args {
                    let inner = match arg {
                        perry_hir::CallArg::Expr(expr) | perry_hir::CallArg::Spread(expr) => expr,
                    };
                    self.collect_expr(inner);
                }
            }
            Expr::Await(operand)
            | Expr::Yield {
                value: Some(operand),
                ..
            }
            | Expr::QueueMicrotask(operand) => {
                self.mark_async_microtask_escape();
                self.collect_expr(operand);
            }
            Expr::Closure { .. } => {
                self.mark_unknown_call_escape();
                perry_hir::walker::walk_expr_children(expr, &mut |child| {
                    self.collect_expr(child);
                });
            }
            _ => {
                perry_hir::walker::walk_expr_children(expr, &mut |child| {
                    self.collect_expr(child);
                });
            }
        }
    }

    fn finish(mut self) -> (ArrayFacts, EffectFacts, MaterializationHazardFacts) {
        let aliases = self.aliases.clone();
        for (alias, root) in aliases {
            if self.materialization_hazard_locals.contains(&root)
                || self.materialization_hazard_locals.contains(&alias)
            {
                self.materialization_hazard_locals.insert(root);
                self.materialization_hazard_locals.insert(alias);
            }
            if self.length_mutation_locals.contains(&root)
                || self.length_mutation_locals.contains(&alias)
            {
                self.length_mutation_locals.insert(root);
                self.length_mutation_locals.insert(alias);
            }
            self.aliased_locals.insert(root);
            self.aliased_locals.insert(alias);
        }

        let length_stable_locals = self
            .local_kinds
            .keys()
            .copied()
            .filter(|id| {
                !self.length_mutation_locals.contains(id)
                    && !self.materialization_hazard_locals.contains(id)
            })
            .collect();
        let noalias_locals = self
            .local_kinds
            .keys()
            .copied()
            .filter(|id| !self.aliased_locals.contains(id))
            .collect();

        (
            ArrayFacts {
                local_kinds: self.local_kinds,
                length_stable_locals,
                noalias_locals,
            },
            EffectFacts {
                unknown_call_escape: self.unknown_call_escape,
                async_microtask_escape: self.async_microtask_escape,
                array_length_mutation_locals: self.length_mutation_locals,
            },
            MaterializationHazardFacts {
                initially_known_hazard_locals: self.materialization_hazard_locals,
            },
        )
    }

    fn record_local_alias_write(&mut self, target_id: u32, value: &Expr) {
        if let Expr::LocalGet(source_id) = value {
            let source_root = self.array_alias_root(*source_id);
            if self.local_kinds.contains_key(&source_root)
                || self.local_kinds.contains_key(&target_id)
            {
                if source_root != target_id {
                    self.aliases.insert(target_id, source_root);
                    self.aliased_locals.insert(source_root);
                    self.aliased_locals.insert(target_id);
                }
                return;
            }
        }
        self.aliases.remove(&target_id);
    }

    fn array_alias_root(&self, mut id: u32) -> u32 {
        let mut seen = HashSet::new();
        while let Some(next) = self.aliases.get(&id).copied() {
            if !seen.insert(id) {
                break;
            }
            id = next;
        }
        id
    }

    fn tracked_array_root(&self, id: u32) -> Option<u32> {
        let root = self.array_alias_root(id);
        if self.local_kinds.contains_key(&root) {
            Some(root)
        } else if self.local_kinds.contains_key(&id) {
            Some(id)
        } else {
            None
        }
    }

    fn mark_array_length_mutation(&mut self, id: u32, observed: ArrayKindFact) {
        if let Some(root) = self.tracked_array_root(id) {
            self.length_mutation_locals.insert(root);
            self.length_mutation_locals.insert(id);
            self.update_array_kind_for_local(root, observed);
            if id != root {
                self.update_array_kind_for_local(id, observed);
            }
        }
    }

    fn mark_array_materialization_hazard(&mut self, id: u32) {
        if let Some(root) = self.tracked_array_root(id) {
            self.materialization_hazard_locals.insert(root);
            self.materialization_hazard_locals.insert(id);
        }
    }

    fn mark_array_target_materialization_hazard(&mut self, target: &Expr) {
        if let Expr::LocalGet(id) = target {
            self.mark_array_materialization_hazard(*id);
            self.update_array_kind_for_local(*id, ArrayKindFact::Unknown);
        }
    }

    fn mark_array_identity_exposure(&mut self, expr: &Expr) {
        match expr {
            Expr::LocalGet(id) => {
                self.mark_array_materialization_hazard(*id);
            }
            Expr::LocalSet(_, value) => self.mark_array_identity_exposure(value),
            Expr::Sequence(exprs) => {
                if let Some(last) = exprs.last() {
                    self.mark_array_identity_exposure(last);
                }
            }
            Expr::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                self.mark_array_identity_exposure(then_expr);
                self.mark_array_identity_exposure(else_expr);
            }
            _ => {}
        }
    }

    fn mark_unknown_call_escape(&mut self) {
        self.unknown_call_escape = true;
        let ids: Vec<u32> = self.local_kinds.keys().copied().collect();
        for id in ids {
            // #6011: param-seeded facts still lose their packed kind on an
            // unknown call (conservative for every fact consumer), but do NOT
            // gain a materialization hazard — params were never hazard-tracked
            // before seeding existed, and hazards feed non-fact consumers
            // (`array_length_receiver_is_loop_local`'s length-hoist gate) that
            // must not regress for `i < param.length` loops in call-bearing
            // bodies. Explicit hazards (freeze/defineProperty/identity escape
            // on the param itself) still mark normally.
            if !self.param_seeded_locals.contains(&id) {
                self.mark_array_materialization_hazard(id);
            }
            self.update_array_kind_for_local(id, ArrayKindFact::Unknown);
        }
    }

    fn mark_async_microtask_escape(&mut self) {
        self.async_microtask_escape = true;
        self.mark_unknown_call_escape();
    }

    fn update_array_kind_for_local(&mut self, id: u32, observed: ArrayKindFact) {
        if let Some(root) = self.tracked_array_root(id) {
            if let Some(kind) = self.local_kinds.get_mut(&root) {
                *kind = meet_array_kind(*kind, observed);
            }
        }
        if let Some(kind) = self.local_kinds.get_mut(&id) {
            *kind = meet_array_kind(*kind, observed);
        }
    }
}

fn array_kind_from_declared_type(ty: &perry_types::Type) -> ArrayKindFact {
    match ty {
        perry_types::Type::Array(elem) if matches!(elem.as_ref(), perry_types::Type::Int32) => {
            ArrayKindFact::PackedI32
        }
        perry_types::Type::Array(elem) if matches!(elem.as_ref(), perry_types::Type::Named(name) if name == "PerryU32") => {
            ArrayKindFact::PackedU32
        }
        perry_types::Type::Array(elem) if matches!(elem.as_ref(), perry_types::Type::Number) => {
            ArrayKindFact::PackedF64
        }
        perry_types::Type::Array(_) => ArrayKindFact::PackedValue,
        // #6011: `new Array<number>(n)` declares/infers as
        // `Generic { base: "Array", type_args: [Number] }` rather than
        // `Array(Number)` — classify the generic spelling identically.
        perry_types::Type::Generic { base, type_args }
            if base == "Array" && type_args.len() == 1 =>
        {
            match &type_args[0] {
                perry_types::Type::Int32 => ArrayKindFact::PackedI32,
                perry_types::Type::Named(name) if name == "PerryU32" => ArrayKindFact::PackedU32,
                perry_types::Type::Number => ArrayKindFact::PackedF64,
                _ => ArrayKindFact::PackedValue,
            }
        }
        perry_types::Type::Generic { base, .. } if base == "Array" => ArrayKindFact::PackedValue,
        _ => ArrayKindFact::Unknown,
    }
}

fn array_kind_from_initializer(expr: &Expr) -> ArrayKindFact {
    match expr {
        // #6011: `new Array()` / `new Array(n)` allocations. Zero args is an
        // empty array; one arg is (almost always) a length — all slots start
        // as TAG_HOLE, which the hole-tolerant packed-f64 range guard accepts.
        // A single NON-numeric arg (`new Array("x")`) actually stores the arg
        // at element 0, making the array non-numeric — that is still fine to
        // seed as PackedF64 because this fact is only a versioning hint: the
        // runtime guard revalidates every slot at loop entry and falls back
        // to the slow loop. Multiple args become elements, so they must all
        // be literal numbers for the packed-f64 seed.
        Expr::New {
            class_name, args, ..
        } if class_name == "Array" => {
            if args.len() <= 1 || args.iter().all(expr_is_literal_number) {
                ArrayKindFact::PackedF64
            } else {
                ArrayKindFact::PackedValue
            }
        }
        Expr::Array(elements) if elements.iter().all(expr_is_literal_i32) => {
            ArrayKindFact::PackedI32
        }
        Expr::Array(elements) if elements.iter().all(expr_is_literal_u32) => {
            ArrayKindFact::PackedU32
        }
        Expr::Array(elements) if elements.iter().all(expr_is_literal_number) => {
            ArrayKindFact::PackedF64
        }
        Expr::Array(_) => ArrayKindFact::PackedValue,
        Expr::ArraySpread(elements) => {
            let mut saw_hole = false;
            let mut all_numeric = true;
            for element in elements {
                match element {
                    perry_hir::ArrayElement::Expr(expr) => {
                        all_numeric &= expr_is_literal_number(expr);
                    }
                    perry_hir::ArrayElement::Spread(_) => return ArrayKindFact::Unknown,
                    perry_hir::ArrayElement::Hole => saw_hole = true,
                }
            }
            if saw_hole {
                ArrayKindFact::HoleyValue
            } else if elements.iter().all(|element| {
                matches!(
                    element,
                    perry_hir::ArrayElement::Expr(expr) if expr_is_literal_i32(expr)
                )
            }) {
                ArrayKindFact::PackedI32
            } else if elements.iter().all(|element| {
                matches!(
                    element,
                    perry_hir::ArrayElement::Expr(expr) if expr_is_literal_u32(expr)
                )
            }) {
                ArrayKindFact::PackedU32
            } else if all_numeric {
                ArrayKindFact::PackedF64
            } else {
                ArrayKindFact::PackedValue
            }
        }
        _ => ArrayKindFact::Unknown,
    }
}

fn array_kind_from_declared_initializer(declared: ArrayKindFact, init: &Expr) -> ArrayKindFact {
    if declared == ArrayKindFact::PackedU32 {
        return if initializer_is_literal_u32_array(init) {
            ArrayKindFact::PackedU32
        } else {
            match array_kind_from_initializer(init) {
                ArrayKindFact::Unknown => ArrayKindFact::Unknown,
                ArrayKindFact::PackedValue => ArrayKindFact::PackedValue,
                ArrayKindFact::HoleyValue => ArrayKindFact::HoleyValue,
                ArrayKindFact::PackedI32 | ArrayKindFact::PackedU32 | ArrayKindFact::PackedF64 => {
                    ArrayKindFact::PackedF64
                }
            }
        };
    }
    meet_declared_array_kind(declared, array_kind_from_initializer(init))
}

fn initializer_is_literal_u32_array(expr: &Expr) -> bool {
    match expr {
        Expr::Array(elements) => elements.iter().all(expr_is_literal_u32),
        Expr::ArraySpread(elements) => elements.iter().all(|element| {
            matches!(
                element,
                perry_hir::ArrayElement::Expr(expr) if expr_is_literal_u32(expr)
            )
        }),
        _ => false,
    }
}

fn expr_is_literal_number(expr: &Expr) -> bool {
    matches!(expr, Expr::Integer(_) | Expr::Number(_))
}

fn expr_is_literal_i32(expr: &Expr) -> bool {
    match expr {
        Expr::Integer(n) => i32::try_from(*n).is_ok(),
        Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => {
            let value = *n as i64;
            i32::try_from(value).is_ok() && *n == value as f64
        }
        _ => false,
    }
}

fn expr_is_literal_u32(expr: &Expr) -> bool {
    match expr {
        Expr::Integer(n) => u32::try_from(*n).is_ok(),
        Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => {
            let value = *n as i64;
            u32::try_from(value).is_ok() && *n == value as f64
        }
        _ => false,
    }
}

fn expr_is_i32_shaped(expr: &Expr) -> bool {
    match expr {
        Expr::Integer(n) => i32::try_from(*n).is_ok(),
        Expr::Binary { op, left, right }
            if matches!(
                op,
                perry_hir::BinaryOp::BitAnd
                    | perry_hir::BinaryOp::BitOr
                    | perry_hir::BinaryOp::BitXor
                    | perry_hir::BinaryOp::Shl
                    | perry_hir::BinaryOp::Shr
                    | perry_hir::BinaryOp::UShr
            ) =>
        {
            expr_is_numeric_shaped(left) && expr_is_numeric_shaped(right)
        }
        Expr::MathImul(left, right) => {
            expr_is_numeric_shaped(left) && expr_is_numeric_shaped(right)
        }
        _ => false,
    }
}

fn expr_is_numeric_shaped(expr: &Expr) -> bool {
    match expr {
        Expr::Integer(_) | Expr::Number(_) | Expr::LocalGet(_) | Expr::IndexGet { .. } => true,
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            expr_is_numeric_shaped(left) && expr_is_numeric_shaped(right)
        }
        Expr::Unary { operand, .. } | Expr::NumberCoerce(operand) | Expr::Void(operand) => {
            expr_is_numeric_shaped(operand)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_is_numeric_shaped(condition)
                && expr_is_numeric_shaped(then_expr)
                && expr_is_numeric_shaped(else_expr)
        }
        Expr::MathImul(left, right) | Expr::MathPow(left, right) => {
            expr_is_numeric_shaped(left) && expr_is_numeric_shaped(right)
        }
        Expr::MathMin(values) | Expr::MathMax(values) => values.iter().all(expr_is_numeric_shaped),
        Expr::MathAbs(value)
        | Expr::MathSqrt(value)
        | Expr::MathFloor(value)
        | Expr::MathCeil(value)
        | Expr::MathRound(value)
        | Expr::MathTrunc(value)
        | Expr::MathSign(value)
        | Expr::MathF16round(value) => expr_is_numeric_shaped(value),
        _ => false,
    }
}

fn meet_array_kind(left: ArrayKindFact, right: ArrayKindFact) -> ArrayKindFact {
    use ArrayKindFact::*;
    match (left, right) {
        (Unknown, _) | (_, Unknown) => Unknown,
        (HoleyValue, _) | (_, HoleyValue) => HoleyValue,
        (PackedValue, _) | (_, PackedValue) => PackedValue,
        (PackedI32, PackedI32) => PackedI32,
        (PackedU32, PackedU32) => PackedU32,
        (PackedI32, PackedF64) | (PackedF64, PackedI32) => PackedF64,
        (PackedU32, PackedF64) | (PackedF64, PackedU32) => PackedF64,
        (PackedI32, PackedU32) | (PackedU32, PackedI32) => PackedF64,
        (PackedF64, PackedF64) => PackedF64,
    }
}

fn meet_declared_array_kind(declared: ArrayKindFact, init: ArrayKindFact) -> ArrayKindFact {
    use ArrayKindFact::*;
    match (declared, init) {
        (PackedU32, PackedU32) => PackedU32,
        (PackedU32, PackedI32) => PackedF64,
        (PackedU32, PackedF64) => PackedF64,
        (PackedI32, PackedU32) => PackedF64,
        _ => meet_array_kind(declared, init),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::BinaryOp;
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

    fn number_array_let(id: u32, values: &[i64]) -> Stmt {
        Stmt::Let {
            id,
            name: format!("a{}", id),
            ty: Type::Array(Box::new(Type::Number)),
            mutable: true,
            init: Some(Expr::Array(
                values.iter().copied().map(Expr::Integer).collect(),
            )),
        }
    }

    fn int32_array_let(id: u32, values: &[i64]) -> Stmt {
        Stmt::Let {
            id,
            name: format!("a{}", id),
            ty: Type::Array(Box::new(Type::Int32)),
            mutable: true,
            init: Some(Expr::Array(
                values.iter().copied().map(Expr::Integer).collect(),
            )),
        }
    }

    fn u32_array_let(id: u32, values: &[i64]) -> Stmt {
        Stmt::Let {
            id,
            name: format!("a{}", id),
            ty: Type::Array(Box::new(Type::Named("PerryU32".to_string()))),
            mutable: true,
            init: Some(Expr::Array(
                values.iter().copied().map(Expr::Integer).collect(),
            )),
        }
    }

    fn alias_let(id: u32, source_id: u32) -> Stmt {
        Stmt::Let {
            id,
            name: format!("alias{}", id),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::LocalGet(source_id)),
        }
    }

    fn dynamic_call() -> Expr {
        Expr::Call {
            callee: Box::new(Expr::LocalGet(99)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        }
    }

    fn ushr0(left: Expr) -> Expr {
        Expr::Binary {
            op: BinaryOp::UShr,
            left: Box::new(left),
            right: Box::new(Expr::Integer(0)),
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
        assert!(facts.proves_unsigned_i32_lowering(2));
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
            &[],
            &HashSet::new(),
            &pure_helpers,
            &HashSet::new(),
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &constants,
        );

        assert!(graph.known_noalias_buffer_locals().contains(&1));
        assert!(graph.proves_noalias_buffer(1));
        assert_eq!(graph.compile_time_constants().get(&90), Some(&1.0));
        assert_eq!(graph.platform_constant(90), Some(1.0));
        assert!(graph.purity.pure_helper_function_ids.contains(&7));
        assert!(graph.proves_pure_helper(7));
    }

    #[test]
    fn packed_i32_array_fact_requires_int32_array_with_i32_literal_initializer() {
        let facts = collect_hir_facts(
            &[
                int32_array_let(1, &[1, 2, 3]),
                number_array_let(2, &[1, 2, 3]),
                Stmt::Let {
                    id: 3,
                    name: "fractional".to_string(),
                    ty: Type::Array(Box::new(Type::Int32)),
                    mutable: true,
                    init: Some(Expr::Array(vec![Expr::Number(1.5)])),
                },
            ],
            &HashSet::new(),
            &HashSet::new(),
        );

        assert_eq!(facts.array_kind(1), ArrayKindFact::PackedI32);
        assert!(facts.proves_packed_i32_array(1));
        assert_eq!(facts.array_kind(2), ArrayKindFact::PackedF64);
        assert!(!facts.proves_packed_i32_array(2));
        assert_eq!(facts.array_kind(3), ArrayKindFact::PackedF64);
        assert!(!facts.proves_packed_i32_array(3));
    }

    #[test]
    fn packed_u32_array_fact_requires_perry_u32_array_with_u32_literal_initializer() {
        let facts = collect_hir_facts(
            &[
                u32_array_let(1, &[0, 4_000_000_000]),
                int32_array_let(2, &[0, 1]),
                Stmt::Let {
                    id: 3,
                    name: "negative".to_string(),
                    ty: Type::Array(Box::new(Type::Named("PerryU32".to_string()))),
                    mutable: true,
                    init: Some(Expr::Array(vec![Expr::Integer(-1)])),
                },
            ],
            &HashSet::new(),
            &HashSet::new(),
        );

        assert_eq!(facts.array_kind(1), ArrayKindFact::PackedU32);
        assert!(facts.proves_packed_u32_array(1));
        assert_eq!(facts.array_kind(2), ArrayKindFact::PackedI32);
        assert!(!facts.proves_packed_u32_array(2));
        assert_eq!(facts.array_kind(3), ArrayKindFact::PackedF64);
        assert!(!facts.proves_packed_u32_array(3));
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
            &[],
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        assert!(graph.integer_locals().contains(&1));
        assert!(graph.proves_i32_lowering(1));
        assert!(graph.proves_bounds_range_seed(1));
        assert!(graph.index_used_locals().contains(&1));
        assert!(graph.non_escaping_object_literals().contains_key(&3));
        assert!(graph
            .shape_stability
            .scalar_replaceable_object_locals
            .contains(&3));
        assert!(graph.scalar_replaceable_object_locals().contains(&3));
        assert!(graph.proves_scalar_replacement(3));
        assert!(!graph.has_materialization_hazard(3));
    }

    #[test]
    fn numeric_array_literal_gets_noalias_length_stable_packed_f64_proof() {
        let graph = collect_hir_facts(
            &[number_array_let(1, &[1, 2, 3])],
            &HashSet::new(),
            &HashSet::new(),
        );

        assert_eq!(graph.array_kind(1), ArrayKindFact::PackedF64);
        assert!(graph.proves_noalias_array(1));
        assert!(graph.proves_array_length_stable(1));
        assert!(!graph.has_materialization_hazard(1));
        assert!(graph.proves_packed_f64_array(1));
    }

    #[test]
    fn array_alias_and_grow_mutation_invalidate_packed_f64_proof() {
        let graph = collect_hir_facts(
            &[
                number_array_let(1, &[1, 2, 3]),
                alias_let(2, 1),
                Stmt::Expr(Expr::ArrayPush {
                    array_id: 2,
                    value: Box::new(Expr::Integer(4)),
                }),
            ],
            &HashSet::new(),
            &HashSet::new(),
        );

        assert!(graph.array_length_mutation_locals().contains(&1));
        assert!(!graph.has_materialization_hazard(1));
        assert!(!graph.proves_noalias_array(1));
        assert!(!graph.proves_array_length_stable(1));
        assert!(!graph.proves_packed_f64_array(1));
    }

    #[test]
    fn non_mutating_array_alias_drops_noalias_but_not_length_stability() {
        let graph = collect_hir_facts(
            &[number_array_let(1, &[1, 2, 3]), alias_let(2, 1)],
            &HashSet::new(),
            &HashSet::new(),
        );

        assert_eq!(graph.array_kind(1), ArrayKindFact::PackedF64);
        assert!(!graph.proves_noalias_array(1));
        assert!(graph.proves_array_length_stable(1));
        assert!(!graph.has_materialization_hazard(1));
        assert!(!graph.proves_packed_f64_array(1));
    }

    #[test]
    fn alias_index_set_invalidates_root_array_length_stability() {
        let graph = collect_hir_facts(
            &[
                number_array_let(1, &[1, 2, 3]),
                alias_let(2, 1),
                Stmt::Expr(Expr::IndexSet {
                    object: Box::new(Expr::LocalGet(2)),
                    index: Box::new(Expr::Integer(0)),
                    value: Box::new(Expr::Integer(9)),
                }),
            ],
            &HashSet::new(),
            &HashSet::new(),
        );

        assert!(graph.array_length_mutation_locals().contains(&1));
        assert!(graph.array_length_mutation_locals().contains(&2));
        assert!(!graph.proves_array_length_stable(1));
        assert!(!graph.has_materialization_hazard(1));
        assert!(!graph.proves_packed_f64_array(1));
    }

    #[test]
    fn direct_array_index_set_invalidates_length_stability_not_materialization() {
        let graph = collect_hir_facts(
            &[
                number_array_let(1, &[1, 2, 3]),
                Stmt::Expr(Expr::IndexSet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::Integer(0)),
                    value: Box::new(Expr::Integer(9)),
                }),
            ],
            &HashSet::new(),
            &HashSet::new(),
        );

        assert!(graph.array_length_mutation_locals().contains(&1));
        assert!(!graph.proves_array_length_stable(1));
        assert!(!graph.has_materialization_hazard(1));
        assert!(!graph.proves_packed_f64_array(1));
    }

    #[test]
    fn aggregate_array_identity_exposure_marks_materialization_hazard() {
        let graph = collect_hir_facts(
            &[
                number_array_let(1, &[1, 2, 3]),
                Stmt::Let {
                    id: 2,
                    name: "box".to_string(),
                    ty: Type::Array(Box::new(Type::Array(Box::new(Type::Number)))),
                    mutable: false,
                    init: Some(Expr::Array(vec![Expr::LocalGet(1)])),
                },
            ],
            &HashSet::new(),
            &HashSet::new(),
        );

        assert!(graph.has_materialization_hazard(1));
        assert!(!graph.proves_array_length_stable(1));
        assert!(!graph.proves_packed_f64_array(1));
    }

    #[test]
    fn unknown_call_escape_marks_array_materialization_hazard() {
        let graph = collect_hir_facts(
            &[number_array_let(1, &[1, 2, 3]), Stmt::Expr(dynamic_call())],
            &HashSet::new(),
            &HashSet::new(),
        );

        assert!(graph.has_unknown_call_escape());
        assert!(graph.has_materialization_hazard(1));
        assert!(!graph.proves_array_length_stable(1));
        assert!(!graph.proves_packed_f64_array(1));
    }

    #[test]
    fn async_microtask_escape_is_tracked_as_effect_fact() {
        let graph = collect_hir_facts(
            &[
                number_array_let(1, &[1, 2, 3]),
                Stmt::Expr(Expr::Await(Box::new(Expr::Undefined))),
            ],
            &HashSet::new(),
            &HashSet::new(),
        );

        assert!(graph.has_async_microtask_escape());
        assert!(graph.has_unknown_call_escape());
        assert!(graph.has_materialization_hazard(1));
        assert!(!graph.proves_array_length_stable(1));
        assert!(!graph.proves_packed_f64_array(1));
    }

    // Regression: a mutable `let __d = undefined` seed (the shape the
    // iterator-protocol array-destructuring lowering emits for each binding
    // element) must NOT leak integer-ness into its immutable `const` copy
    // chain. `cbBase = __d` then `cb = cbBase` previously ended up in
    // `integer_locals` + `strictly_i32_bounded_locals`, giving `cb` an i32
    // shadow slot that fptosi'd a NaN-boxed object/string to i32::MIN
    // (`(number).setName is not a function` in drizzle's column builders).
    #[test]
    fn destructure_undefined_seed_does_not_leak_into_const_copy_chain() {
        // let __d = undefined        (id 1, mutable seed)
        // if (cond) { __d = undefined } else { __d = src.value }  (non-int writes)
        // const cbBase = __d         (id 2)
        // const cb = cbBase          (id 3)
        let stmts = vec![
            mutable_number_let(1, Expr::Undefined),
            Stmt::If {
                condition: Expr::LocalGet(98),
                then_branch: vec![Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::Undefined)))],
                else_branch: Some(vec![Stmt::Expr(Expr::LocalSet(
                    1,
                    Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(99)),
                        property: "value".to_string(),
                    }),
                ))]),
            },
            const_let(2, Expr::LocalGet(1)),
            const_let(3, Expr::LocalGet(2)),
        ];

        let ints = super::super::integer_locals::collect_integer_locals(
            &stmts,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        );

        assert!(
            !ints.contains(&1),
            "mutable undefined seed must be disqualified"
        );
        assert!(
            !ints.contains(&2),
            "const copy of a disqualified seed must not be integer"
        );
        assert!(
            !ints.contains(&3),
            "second-hop const copy must not be integer (the regressing slot)"
        );
    }

    // Guard against over-pruning: a `const` whose source is a *legitimately*
    // integer mutable accumulator (every write `| 0`) must stay in the set so
    // image_convolution-style i32 chains keep their shadow slots.
    #[test]
    fn const_copy_of_live_integer_accumulator_stays_integer() {
        let bitor0 = |left: Expr| Expr::Binary {
            op: BinaryOp::BitOr,
            left: Box::new(left),
            right: Box::new(Expr::Integer(0)),
        };
        // let acc = 0|0 ; acc = (acc) | 0 ; const snap = acc;
        let stmts = vec![
            mutable_number_let(1, bitor0(Expr::Integer(0))),
            Stmt::Expr(Expr::LocalSet(1, Box::new(bitor0(Expr::LocalGet(1))))),
            const_let(2, Expr::LocalGet(1)),
        ];

        let ints = super::super::integer_locals::collect_integer_locals(
            &stmts,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        );

        assert!(ints.contains(&1), "live |0 accumulator must stay integer");
        assert!(
            ints.contains(&2),
            "const copy of a live integer accumulator must stay integer"
        );
    }

    // Regression (parameter-destructuring path): the bindings the *param*
    // destructure lowering emits are `mutable: true` with no reassignment, so
    // they escape an immutable-only re-validation. They must still be pruned
    // via their init-only definition when their `undefined`-seed source is
    // disqualified.
    #[test]
    fn destructure_mutable_param_bindings_do_not_leak_into_copy() {
        // let __d = undefined           (id 1, mutable seed, has non-int writes)
        // __d = src.value
        // let cbBase = __d              (id 2, mutable binding, NO LocalSet)
        // const cb = cbBase             (id 3)
        let stmts = vec![
            mutable_number_let(1, Expr::Undefined),
            Stmt::Expr(Expr::LocalSet(
                1,
                Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(99)),
                    property: "value".to_string(),
                }),
            )),
            mutable_number_let(2, Expr::LocalGet(1)),
            const_let(3, Expr::LocalGet(2)),
        ];

        let ints = super::super::integer_locals::collect_integer_locals(
            &stmts,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        );

        assert!(
            !ints.contains(&1),
            "mutable undefined seed must be disqualified"
        );
        assert!(
            !ints.contains(&2),
            "mutable param binding copied from a disqualified seed must not be integer"
        );
        assert!(
            !ints.contains(&3),
            "const copy of the mutable param binding must not be integer"
        );
    }

    // Provenance hole #1 (the clamp_fn_ids bypass): a local admitted via a
    // clamp3-shaped call must be pruned when an *argument* of that call is
    // disqualified — clamp3 returns one of its arguments verbatim, so the
    // result is only an integer if the arguments are. Previously
    // `is_int32_producing_expr` accepted any clamp call unconditionally, so
    // the candidate kept its i32 slot forever.
    #[test]
    fn clamp_admitted_local_is_pruned_when_arg_source_is_disqualified() {
        let clamp_call = |arg: Expr| Expr::Call {
            callee: Box::new(Expr::FuncRef(7)),
            args: vec![arg, Expr::Integer(0), Expr::Integer(100)],
            type_args: vec![],
            byte_offset: 0,
        };
        // let src = undefined; src = obj.value;       (disqualified seed)
        // const xx = clamp3(src, 0, 100);             (clamp-admitted)
        // const yy = xx;                              (downstream copy)
        let stmts = vec![
            mutable_number_let(1, Expr::Undefined),
            Stmt::Expr(Expr::LocalSet(
                1,
                Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(99)),
                    property: "value".to_string(),
                }),
            )),
            const_let(2, clamp_call(Expr::LocalGet(1))),
            const_let(3, Expr::LocalGet(2)),
        ];
        let clamp_ids: HashSet<u32> = [7].into_iter().collect();

        let ints = super::super::integer_locals::collect_integer_locals(
            &stmts,
            &HashSet::new(),
            &clamp_ids,
            &clamp_ids,
        );
        assert!(!ints.contains(&1), "non-int-written seed must be pruned");
        assert!(
            !ints.contains(&2),
            "clamp3-admitted local must follow its disqualified argument"
        );
        assert!(
            !ints.contains(&3),
            "copy of the clamp3-admitted local must be pruned transitively"
        );

        // Same shape with integer-stable arguments keeps the optimization.
        let ok_stmts = vec![
            mutable_number_let(1, Expr::Integer(5)),
            const_let(2, clamp_call(Expr::LocalGet(1))),
            const_let(3, Expr::LocalGet(2)),
        ];
        let ints = super::super::integer_locals::collect_integer_locals(
            &ok_stmts,
            &HashSet::new(),
            &clamp_ids,
            &clamp_ids,
        );
        assert!(ints.contains(&2), "int-arg clamp3 result must stay integer");
        assert!(ints.contains(&3), "copy of live clamp3 result must stay");

        // Argument-INdependent clamp functions (clampU8 / returns_integer —
        // they coerce internally) must keep admitting double-valued args.
        let coercing_stmts = vec![const_let(2, clamp_call(Expr::LocalGet(98)))];
        let ints = super::super::integer_locals::collect_integer_locals(
            &coercing_stmts,
            &HashSet::new(),
            &clamp_ids,
            &HashSet::new(),
        );
        assert!(
            ints.contains(&2),
            "internally-coercing clamp result must stay integer regardless of args"
        );
    }

    // Provenance hole #2 (init bypass on written locals): a candidate WITH
    // `LocalSet` writes was never re-validated through its init, so
    // `let b = a; …use b…; b = 1` kept b integer after `a` was disqualified
    // — reads between the init and the int write saw a truncated pointer.
    #[test]
    fn written_local_is_still_revalidated_through_its_init() {
        // let a = undefined; a = obj.value;   (disqualified seed)
        // let b = a;                          (init copies disqualified a)
        // b = 1;                              (later int write)
        let stmts = vec![
            mutable_number_let(1, Expr::Undefined),
            Stmt::Expr(Expr::LocalSet(
                1,
                Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(99)),
                    property: "value".to_string(),
                }),
            )),
            mutable_number_let(2, Expr::LocalGet(1)),
            Stmt::Expr(Expr::LocalSet(2, Box::new(Expr::Integer(1)))),
        ];

        let ints = super::super::integer_locals::collect_integer_locals(
            &stmts,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(
            !ints.contains(&2),
            "a written local whose init copies a disqualified source must be pruned"
        );
    }

    // Provenance hole #3 (Update bypass): `const y = x++` was unconditionally
    // int-producing even when `x` never was (or stopped being) an integer.
    #[test]
    fn update_admitted_local_follows_its_target() {
        // let x = undefined; x = obj.value; const y = x++;
        let stmts = vec![
            mutable_number_let(1, Expr::Undefined),
            Stmt::Expr(Expr::LocalSet(
                1,
                Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(99)),
                    property: "value".to_string(),
                }),
            )),
            const_let(
                2,
                Expr::Update {
                    id: 1,
                    op: perry_hir::UpdateOp::Increment,
                    prefix: false,
                },
            ),
        ];

        let ints = super::super::integer_locals::collect_integer_locals(
            &stmts,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(
            !ints.contains(&2),
            "`x++` over a disqualified local must not stay integer"
        );
    }
}
