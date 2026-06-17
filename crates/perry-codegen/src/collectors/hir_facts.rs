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
pub(crate) struct TypeFacts {
    pub representation: RepresentationFacts,
    pub arrays: ArrayFacts,
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
    PackedF64,
    PackedValue,
    HoleyValue,
    Unknown,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ArrayFacts {
    pub local_kinds: HashMap<u32, ArrayKindFact>,
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
    pub non_escaping_object_literals: HashMap<u32, Vec<String>>,
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

    pub(crate) fn non_escaping_object_literals(&self) -> &HashMap<u32, Vec<String>> {
        &self.escape.non_escaping_object_literals
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
    let array_facts = collect_array_facts(stmts);
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
    let non_escaping_object_literals = super::escape_objects::collect_non_escaping_object_literals(
        stmts,
        boxed_vars,
        module_globals,
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
            non_escaping_object_literals,
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_native_region_fact_graph(
    stmts: &[Stmt],
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
    collect_owned_buffer_lets(stmts, &mut out);
    out
}

fn collect_owned_buffer_lets(stmts: &[Stmt], out: &mut HashSet<u32>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                id,
                mutable,
                init: Some(init),
                ..
            } => {
                if !*mutable && is_owned_u8_buffer_alloc(init) {
                    out.insert(*id);
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_owned_buffer_lets(then_branch, out);
                if let Some(else_branch) = else_branch {
                    collect_owned_buffer_lets(else_branch, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_owned_buffer_lets(body, out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    collect_owned_buffer_lets(std::slice::from_ref(init.as_ref()), out);
                }
                collect_owned_buffer_lets(body, out);
            }
            Stmt::Labeled { body, .. } => {
                collect_owned_buffer_lets(std::slice::from_ref(body.as_ref()), out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_owned_buffer_lets(body, out);
                if let Some(catch) = catch {
                    collect_owned_buffer_lets(&catch.body, out);
                }
                if let Some(finally) = finally {
                    collect_owned_buffer_lets(finally, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_owned_buffer_lets(&case.body, out);
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

fn is_owned_u8_buffer_alloc(expr: &Expr) -> bool {
    match expr {
        Expr::BufferAlloc { .. } | Expr::BufferAllocUnsafe(_) => true,
        Expr::Uint8ArrayNew(None) => true,
        Expr::Uint8ArrayNew(Some(size)) => is_fresh_uint8array_length_literal(size),
        Expr::TypedArrayNew { arg: None, .. } => true,
        Expr::TypedArrayNew {
            arg: Some(size), ..
        } => is_fresh_uint8array_length_literal(size),
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

fn is_fresh_uint8array_length_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Integer(n) => *n >= 0 && *n < i32::MAX as i64,
        Expr::Number(n) => n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n < i32::MAX as f64,
        _ => false,
    }
}

fn collect_array_facts(stmts: &[Stmt]) -> ArrayFacts {
    let mut facts = ArrayFacts::default();
    collect_array_facts_in_stmts(stmts, &mut facts.local_kinds);
    facts
}

fn collect_array_facts_in_stmts(stmts: &[Stmt], kinds: &mut HashMap<u32, ArrayKindFact>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { id, ty, init, .. } => {
                let declared_kind = array_kind_from_declared_type(ty);
                if declared_kind != ArrayKindFact::Unknown {
                    let init_kind = init
                        .as_ref()
                        .map(array_kind_from_initializer)
                        .unwrap_or(ArrayKindFact::Unknown);
                    kinds.insert(*id, meet_array_kind(declared_kind, init_kind));
                }
                if let Some(init) = init {
                    collect_array_facts_in_expr(init, kinds);
                }
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                collect_array_facts_in_expr(expr, kinds);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_array_facts_in_expr(condition, kinds);
                collect_array_facts_in_stmts(then_branch, kinds);
                if let Some(else_branch) = else_branch {
                    collect_array_facts_in_stmts(else_branch, kinds);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                collect_array_facts_in_expr(condition, kinds);
                collect_array_facts_in_stmts(body, kinds);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    collect_array_facts_in_stmts(std::slice::from_ref(init.as_ref()), kinds);
                }
                if let Some(condition) = condition {
                    collect_array_facts_in_expr(condition, kinds);
                }
                if let Some(update) = update {
                    collect_array_facts_in_expr(update, kinds);
                }
                collect_array_facts_in_stmts(body, kinds);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_array_facts_in_stmts(body, kinds);
                if let Some(catch) = catch {
                    collect_array_facts_in_stmts(&catch.body, kinds);
                }
                if let Some(finally) = finally {
                    collect_array_facts_in_stmts(finally, kinds);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                collect_array_facts_in_expr(discriminant, kinds);
                for case in cases {
                    if let Some(test) = &case.test {
                        collect_array_facts_in_expr(test, kinds);
                    }
                    collect_array_facts_in_stmts(&case.body, kinds);
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_array_facts_in_stmts(std::slice::from_ref(body.as_ref()), kinds);
            }
            Stmt::Return(None)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_) => {}
        }
    }
}

fn collect_array_facts_in_expr(expr: &Expr, kinds: &mut HashMap<u32, ArrayKindFact>) {
    match expr {
        Expr::ArrayPush { array_id, value } => {
            let value_kind = if expr_is_numeric_shaped(value) {
                ArrayKindFact::PackedF64
            } else {
                ArrayKindFact::PackedValue
            };
            update_array_kind(kinds, *array_id, value_kind);
            collect_array_facts_in_expr(value, kinds);
        }
        Expr::ArrayPushSpread { array_id, source } => {
            update_array_kind(kinds, *array_id, ArrayKindFact::Unknown);
            collect_array_facts_in_expr(source, kinds);
        }
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
            update_array_kind(kinds, *id, ArrayKindFact::HoleyValue);
        }
        Expr::ArrayUnshift { array_id, value } => {
            update_array_kind(kinds, *array_id, ArrayKindFact::Unknown);
            collect_array_facts_in_expr(value, kinds);
        }
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => {
            update_array_kind(kinds, *array_id, ArrayKindFact::Unknown);
            collect_array_facts_in_expr(start, kinds);
            if let Some(delete_count) = delete_count {
                collect_array_facts_in_expr(delete_count, kinds);
            }
            for item in items {
                collect_array_facts_in_expr(item, kinds);
            }
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                let value_kind = if expr_is_numeric_shaped(value) {
                    ArrayKindFact::PackedF64
                } else {
                    ArrayKindFact::PackedValue
                };
                update_array_kind(kinds, *id, value_kind);
            }
            collect_array_facts_in_expr(object, kinds);
            collect_array_facts_in_expr(index, kinds);
            collect_array_facts_in_expr(value, kinds);
        }
        Expr::LocalSet(id, value) => {
            if kinds.contains_key(id) {
                update_array_kind(kinds, *id, ArrayKindFact::Unknown);
            }
            collect_array_facts_in_expr(value, kinds);
        }
        Expr::ObjectFreeze(target)
        | Expr::ObjectSeal(target)
        | Expr::ObjectPreventExtensions(target) => {
            invalidate_array_kind_target(kinds, target);
            collect_array_facts_in_expr(target, kinds);
        }
        Expr::ObjectDefineProperty(target, key, descriptor)
        | Expr::ReflectDefineProperty {
            target,
            key,
            descriptor,
        } => {
            invalidate_array_kind_target(kinds, target);
            collect_array_facts_in_expr(target, kinds);
            collect_array_facts_in_expr(key, kinds);
            collect_array_facts_in_expr(descriptor, kinds);
        }
        Expr::ObjectDefineProperties(target, descriptors) => {
            invalidate_array_kind_target(kinds, target);
            collect_array_facts_in_expr(target, kinds);
            collect_array_facts_in_expr(descriptors, kinds);
        }
        Expr::ObjectSetPrototypeOf(target, proto) => {
            invalidate_array_kind_target(kinds, target);
            collect_array_facts_in_expr(target, kinds);
            collect_array_facts_in_expr(proto, kinds);
        }
        Expr::Call { callee, args, .. } => {
            collect_array_facts_in_expr(callee, kinds);
            for arg in args {
                if let Expr::LocalGet(id) = arg {
                    if kinds.contains_key(id) {
                        update_array_kind(kinds, *id, ArrayKindFact::Unknown);
                    }
                }
                collect_array_facts_in_expr(arg, kinds);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            collect_array_facts_in_expr(callee, kinds);
            for arg in args {
                let inner = match arg {
                    perry_hir::CallArg::Expr(expr) | perry_hir::CallArg::Spread(expr) => expr,
                };
                if let Expr::LocalGet(id) = inner {
                    if kinds.contains_key(id) {
                        update_array_kind(kinds, *id, ArrayKindFact::Unknown);
                    }
                }
                collect_array_facts_in_expr(inner, kinds);
            }
        }
        Expr::Closure { .. } => {
            for kind in kinds.values_mut() {
                *kind = ArrayKindFact::Unknown;
            }
        }
        _ => {
            perry_hir::walker::walk_expr_children(expr, &mut |child| {
                collect_array_facts_in_expr(child, kinds);
            });
        }
    }
}

fn invalidate_array_kind_target(kinds: &mut HashMap<u32, ArrayKindFact>, target: &Expr) {
    if let Expr::LocalGet(id) = target {
        if kinds.contains_key(id) {
            update_array_kind(kinds, *id, ArrayKindFact::Unknown);
        }
    }
}

fn update_array_kind(kinds: &mut HashMap<u32, ArrayKindFact>, id: u32, observed: ArrayKindFact) {
    if let Some(kind) = kinds.get_mut(&id) {
        *kind = meet_array_kind(*kind, observed);
    }
}

fn array_kind_from_declared_type(ty: &perry_types::Type) -> ArrayKindFact {
    match ty {
        perry_types::Type::Array(elem)
            if matches!(
                elem.as_ref(),
                perry_types::Type::Number | perry_types::Type::Int32
            ) =>
        {
            ArrayKindFact::PackedF64
        }
        perry_types::Type::Array(_) => ArrayKindFact::PackedValue,
        _ => ArrayKindFact::Unknown,
    }
}

fn array_kind_from_initializer(expr: &Expr) -> ArrayKindFact {
    match expr {
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
            } else if all_numeric {
                ArrayKindFact::PackedF64
            } else {
                ArrayKindFact::PackedValue
            }
        }
        _ => ArrayKindFact::Unknown,
    }
}

fn expr_is_literal_number(expr: &Expr) -> bool {
    matches!(expr, Expr::Integer(_) | Expr::Number(_))
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
        (PackedF64, PackedF64) => PackedF64,
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
