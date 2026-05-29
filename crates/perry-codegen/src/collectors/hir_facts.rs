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

impl NativeRegionFactGraph {
    pub(crate) fn integer_locals(&self) -> &HashSet<u32> {
        &self.representation.integer_locals
    }

    pub(crate) fn unsigned_i32_locals(&self) -> &HashSet<u32> {
        &self.representation.unsigned_i32_locals
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
    let graph = NativeRegionFactGraph {
        representation: RepresentationFacts {
            integer_locals: integer_locals.clone(),
            unsigned_i32_locals,
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
}
