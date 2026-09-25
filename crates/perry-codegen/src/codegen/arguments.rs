use std::collections::HashSet;

use perry_hir::{Expr, Param, Stmt};

use crate::block::LlBlock;
use crate::expr::{nanbox_pointer_inline, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64, PTR};

/// Internal-only declared type used by the direct-call clone whose trailing
/// synthetic `arguments` slot carries the already boxed argument count.
/// Source HIR can never name this type: the marker is attached only to a
/// cloned method immediately before codegen.
pub(crate) const SYNTHETIC_ARGUMENTS_LENGTH_TYPE: &str = "__perry_arguments_length_scalar";

/// Additive direct-call ABI for methods proved to observe `arguments` only
/// through exact `.length` reads. The public method keeps its ordinary marked
/// Array/Arguments ABI for runtime dispatch and reflection.
pub(crate) fn arguments_length_method_name(public_name: &str) -> String {
    format!("{public_name}$arguments_length")
}

pub(crate) enum ArgumentsCallee<'a> {
    Undefined,
    FunctionWrapper(&'a str),
    CurrentClosure,
}

/// A mapped parameter is boxed so the sloppy-mode object and the parameter
/// share one cell. An elided object aliases nothing, so its parameters keep
/// ordinary slots — `body` is the same proof input
/// [`materialize_arguments_object`] receives (`None` never elides).
pub(crate) fn add_arguments_mapped_boxes(
    params: &[Param],
    body: Option<&[Stmt]>,
    boxed_vars: &mut HashSet<u32>,
) {
    if body.is_some_and(|body| arguments_elision(params, body).is_some()) {
        return;
    }
    for (_, param_id) in mapped_arguments_params(params) {
        boxed_vars.insert(param_id);
    }
}

pub(crate) fn store_param_slot(
    blk: &mut LlBlock,
    param: &Param,
    boxed_vars: &HashSet<u32>,
    arg_name: &str,
) -> String {
    let boxed_param = boxed_vars.contains(&param.id) && param.arguments_object.is_none();
    let slot = blk.alloca(if boxed_param { I64 } else { DOUBLE });
    if boxed_param {
        let arg_bits = blk.bitcast_double_to_i64(arg_name);
        let box_ptr = blk.call(I64, "js_box_alloc_bits", &[(I64, &arg_bits)]);
        blk.store(I64, &box_ptr, &slot);
    } else {
        blk.store(DOUBLE, arg_name, &slot);
    }
    slot
}

/// #10464: a boxed parameter's cell is minted by this frame's entry block
/// (`store_param_slot`), so the frame releases it before every `ret`.
/// `materialize_arguments_object` withdraws a slot it maps into a sloppy-mode
/// `arguments` object, which holds the raw cell without a counted edge.
pub(crate) fn release_boxed_param_slots_at_exit(
    lf: &mut crate::function::LlFunction,
    params: &[Param],
    boxed_vars: &HashSet<u32>,
    slots: &std::collections::HashMap<u32, String>,
) {
    for p in params {
        if !boxed_vars.contains(&p.id) || p.arguments_object.is_some() {
            continue;
        }
        if let Some(slot) = slots.get(&p.id) {
            lf.add_pre_return_box_release(slot, "js_box_scope_release");
        }
    }
}

/// The parameter ids a synthesized `arguments` object aliases.
pub(crate) fn mapped_parameter_ids(params: &[Param]) -> HashSet<u32> {
    mapped_arguments_params(params)
        .into_iter()
        .map(|(_, id)| id)
        .collect()
}

pub(crate) fn materialize_arguments_object(
    ctx: &mut FnCtx<'_>,
    params: &[Param],
    body: Option<&[Stmt]>,
    callee: ArgumentsCallee<'_>,
) {
    let Some(synth_param) = params.iter().find(|p| p.arguments_object.is_some()) else {
        return;
    };
    let Some(meta) = synth_param.arguments_object.as_ref() else {
        return;
    };
    // Call lowering has already bundled every supplied argument into the
    // synthesized slot as a marked Array. When the body only reads
    // `arguments.length` and `arguments[k]`, that bundle answers both and a
    // full ECMAScript Arguments object would only add allocation, descriptor
    // and registry bookkeeping, and GC pressure (#8807, #10509). Keep the
    // existing conservative materialization path for every other use
    // (including callers that cannot provide a body).
    if body.is_some_and(|body| arguments_elision(params, body).is_some()) {
        // The direct-call clone's slot already carries the scalar count, and
        // property lowering reads it straight off that slot.
        if matches!(
            &synth_param.ty,
            perry_hir::types::Type::Named(name) if name == SYNTHETIC_ARGUMENTS_LENGTH_TYPE
        ) {
            return;
        }
        let Some(arguments_slot) = ctx.locals.get(&synth_param.id).cloned() else {
            return;
        };
        // The bundle is private to this call and nothing below can write it
        // (that is the proof), so its length is read once, here.
        let raw_args = ctx.block().load(DOUBLE, &arguments_slot);
        let raw_bits = ctx.block().bitcast_double_to_i64(&raw_args);
        let len = ctx
            .block()
            .call(I32, "js_array_length", &[(I64, &raw_bits)]);
        let len = ctx.block().uitofp(I32, &len, DOUBLE);
        let length_slot = ctx.func.alloca_entry(DOUBLE);
        ctx.block().store(DOUBLE, &len, &length_slot);
        let callee = match callee {
            ArgumentsCallee::Undefined => ElidedArgumentsCallee::Undefined,
            ArgumentsCallee::FunctionWrapper(wrapper) => {
                ElidedArgumentsCallee::FunctionWrapper(wrapper.to_string())
            }
            ArgumentsCallee::CurrentClosure => ElidedArgumentsCallee::CurrentClosure,
        };
        ctx.elided_arguments.insert(
            synth_param.id,
            ElidedArguments {
                restricted_callee: meta.restricted_callee,
                callee,
                length_slot,
            },
        );
        return;
    }
    let Some(arguments_slot) = ctx.locals.get(&synth_param.id).cloned() else {
        return;
    };
    let restricted = if meta.restricted_callee { "1" } else { "0" };
    let raw_args = ctx.block().load(DOUBLE, &arguments_slot);
    let callee_value = if meta.restricted_callee {
        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
    } else {
        match callee {
            ArgumentsCallee::Undefined => {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            }
            ArgumentsCallee::FunctionWrapper(wrapper) => {
                let wrap_ref = format!("@{}", wrapper);
                let closure_ptr =
                    ctx.block()
                        .call(I64, "js_closure_alloc_singleton", &[(PTR, &wrap_ref)]);
                nanbox_pointer_inline(ctx.block(), &closure_ptr)
            }
            ArgumentsCallee::CurrentClosure => {
                // #7055: through the shadow-rooted slot when there is one — the
                // raw `%this_closure` register is not a GC root.
                let ptr = crate::expr::try_current_closure_ptr_value(ctx)
                    .unwrap_or_else(|| "%this_closure".to_string());
                nanbox_pointer_inline(ctx.block(), &ptr)
            }
        }
    };
    let args_obj = ctx.block().call(
        I64,
        "js_arguments_object_alloc",
        &[
            (DOUBLE, &raw_args),
            (DOUBLE, &callee_value),
            (I32, restricted),
        ],
    );
    for (arg_index, param_id) in mapped_arguments_params(params) {
        if let Some(param_slot) = ctx.locals.get(&param_id).cloned() {
            // #10464: the object aliases the cell for its own lifetime.
            ctx.func.forget_pre_return_box_release(&param_slot);
            let box_ptr = ctx.block().load(I64, &param_slot);
            ctx.block().call_void(
                "js_arguments_object_map_index",
                &[
                    (I64, &args_obj),
                    (I32, &arg_index.to_string()),
                    (I64, &box_ptr),
                ],
            );
        }
    }
    let boxed_args = nanbox_pointer_inline(ctx.block(), &args_obj);
    ctx.block().store(DOUBLE, &boxed_args, &arguments_slot);
}

/// How far the synthesized Arguments object can be replaced by the raw
/// argument bundle call lowering already stores in its slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArgumentsElision {
    /// Every use is an exact `arguments.length` read (#8807).
    LengthOnly,
    /// Every use is an `arguments.length` read or an `arguments[k]` value read
    /// (#10509), no closure captures the binding, and no parameter a sloppy
    /// mapped object would alias can change. Reads lower through
    /// [`try_lower_elided_arguments_index_get`].
    Reads,
}

/// The callee a cold `arguments[k]` read rebuilds the object with — the
/// owned form of [`ArgumentsCallee`] the prologue was handed.
#[derive(Clone, Debug)]
pub(crate) enum ElidedArgumentsCallee {
    Undefined,
    FunctionWrapper(String),
    CurrentClosure,
}

/// A function-local `arguments` binding whose slot holds the raw bundle
/// instead of an Arguments object (either [`ArgumentsElision`]).
#[derive(Clone, Debug)]
pub(crate) struct ElidedArguments {
    pub(crate) restricted_callee: bool,
    pub(crate) callee: ElidedArgumentsCallee,
    /// Entry-block `double` slot holding the bundle's length, stored once by
    /// the prologue.
    pub(crate) length_slot: String,
}

/// Prove that replacing the synthesized Arguments object with its raw argument
/// bundle cannot be observed. The proof is deliberately fail-closed:
///
/// * HIR's canonical local-reference collector counts every use of the
///   synthetic local, including specialized local-bearing expressions such as
///   `ArrayPop(id)`, and that count must equal the `LocalGet`s the generic
///   expression traversal meets — so no use hides in a form this proof does
///   not model, and a traversal blind spot rejects rather than admits.
/// * Every one of those `LocalGet`s must be the receiver of an exact
///   `arguments.length` read or an `arguments[k]` read in value position. A
///   read in reference position — `delete arguments[k]`, or a call whose
///   callee is `arguments[k]` and so receives the object as `this` — rejects.
/// * Index reads additionally need the binding uncaptured (a closure body is
///   lowered without this function's elision state) and, for a sloppy mapped
///   object, every aliased parameter provably still holding its incoming value.
///
/// Parameter defaults are scanned with the body: they evaluate in the
/// function's scope and see the same binding.
pub(crate) fn arguments_elision(params: &[Param], body: &[Stmt]) -> Option<ArgumentsElision> {
    let synth_param = params.iter().find(|p| p.arguments_object.is_some())?;
    let meta = synth_param.arguments_object.as_ref()?;
    let uses = LocalUses::scan(body, params, synth_param.id);
    if !uses.only_value_reads() {
        return None;
    }
    if uses.index_reads == 0 {
        return Some(ArgumentsElision::LengthOnly);
    }
    if uses.captured {
        return None;
    }
    if !meta.mapped_parameter_ids.is_empty() {
        let rebound = crate::collectors::rebound_locals(body);
        for (_, param_id) in &meta.mapped_parameter_ids {
            let param_uses = LocalUses::scan(body, params, *param_id);
            if rebound.contains(param_id) || param_uses.refs != param_uses.local_gets {
                return None;
            }
        }
    }
    Some(ArgumentsElision::Reads)
}

/// Syntactic uses of one local, as [`arguments_elision`] classifies them.
#[derive(Default)]
struct LocalUses {
    /// Every reference the canonical collector sees, including id-bearing
    /// forms (`LocalSet`, `Update`, `ArrayPop`, …) that are not `LocalGet`s.
    refs: usize,
    /// `LocalGet(id)` nodes met by the expression traversal.
    local_gets: usize,
    /// `id.length` reads.
    length_reads: usize,
    /// `id[k]` reads.
    index_reads: usize,
    /// `delete id[k]` / `delete id.p`, or a call whose callee is `id[k]` /
    /// `id.p` (the object becomes `this`).
    reference_uses: usize,
    /// A closure captures or references the local.
    captured: bool,
}

impl LocalUses {
    fn scan(body: &[Stmt], params: &[Param], id: u32) -> Self {
        let defaults: Vec<Stmt> = params
            .iter()
            .filter_map(|p| p.default.clone().map(Stmt::Expr))
            .collect();
        let mut uses = Self::default();
        let mut refs = Vec::new();
        let mut visited_closures = HashSet::new();
        for stmt in body.iter().chain(&defaults) {
            perry_hir::collect_local_refs_stmt(stmt, &mut refs, &mut visited_closures);
        }
        uses.refs = refs.iter().filter(|r| **r == id).count();
        let mut note = |expr: &Expr| uses.note(expr, id);
        crate::collectors::for_each_expr_in_stmts(body, &mut note);
        crate::collectors::for_each_expr_in_stmts(&defaults, &mut note);
        uses
    }

    fn note(&mut self, expr: &Expr, id: u32) {
        let is_local = |e: &Expr| matches!(e, Expr::LocalGet(l) if *l == id);
        let is_member = |e: &Expr| {
            matches!(
                e,
                Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. }
                    if is_local(object)
            )
        };
        match expr {
            Expr::LocalGet(l) if *l == id => self.local_gets += 1,
            Expr::PropertyGet {
                object, property, ..
            } if property == "length" && is_local(object) => self.length_reads += 1,
            Expr::IndexGet { object, .. } if is_local(object) => self.index_reads += 1,
            Expr::Delete(target) if is_member(target) => self.reference_uses += 1,
            Expr::Call { callee, .. } | Expr::CallSpread { callee, .. } if is_member(callee) => {
                self.reference_uses += 1
            }
            Expr::Closure { captures, body, .. } => {
                let mut refs = Vec::new();
                let mut visited_closures = HashSet::new();
                for stmt in body {
                    perry_hir::collect_local_refs_stmt(stmt, &mut refs, &mut visited_closures);
                }
                self.captured |= captures.contains(&id) || refs.contains(&id);
            }
            _ => {}
        }
    }

    fn only_value_reads(&self) -> bool {
        let reads = self.length_reads + self.index_reads;
        reads > 0
            && self.refs == self.local_gets
            && self.local_gets == reads
            && self.reference_uses == 0
    }
}

fn arguments_used_only_for_length(body: &[Stmt], arguments_id: u32) -> bool {
    let uses = LocalUses::scan(body, &[], arguments_id);
    uses.only_value_reads() && uses.index_reads == 0
}

/// `arguments.length` on an elided binding: the bundle length the prologue
/// stored, instead of the property-IC miss a dynamic `.length` would take.
pub(crate) fn lower_elided_arguments_length(ctx: &mut FnCtx<'_>, id: u32) -> Option<String> {
    let slot = ctx.elided_arguments.get(&id)?.length_slot.clone();
    Some(ctx.block().load(DOUBLE, &slot))
}

/// Whether `expr` is an `arguments[k]` read [`try_lower_elided_arguments_index_get`]
/// owns — number-context tiers that would otherwise claim an untyped local's
/// element read must defer to it.
pub(crate) fn is_elided_arguments_index_get(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::IndexGet { object, .. }
            if matches!(object.as_ref(), Expr::LocalGet(id) if ctx.elided_arguments.contains_key(id))
    )
}

/// #10509: `arguments[k]` against an elided Arguments object
/// ([`ArgumentsElision::Reads`]). The slot holds the caller's marked Array, so
/// an own element is one runtime read. Any other key — `"callee"`, a symbol,
/// an inherited name, an out-of-range or fractional number — answers
/// `TAG_HOLE` and takes a cold call that builds the object the prologue would
/// have built and performs an ordinary `[[Get]]` on it, so the result is the
/// same value the materialized object would have produced.
pub(crate) fn try_lower_elided_arguments_index_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> anyhow::Result<Option<String>> {
    let Expr::LocalGet(id) = object else {
        return Ok(None);
    };
    let Some(elided) = ctx.elided_arguments.get(id).cloned() else {
        return Ok(None);
    };
    crate::rooting::with_operands_rooted(ctx, &[object, index], |ctx, vals| {
        let (raw_args, key) = (vals[0].clone(), vals[1].clone());
        let own = ctx.block().call(
            DOUBLE,
            "js_arguments_bundle_index_get",
            &[(DOUBLE, &raw_args), (DOUBLE, &key)],
        );
        let own_bits = ctx.block().bitcast_double_to_i64(&own);
        let not_own = ctx
            .block()
            .icmp_eq(I64, &own_bits, crate::nanbox::TAG_HOLE_I64);
        let slow_idx = ctx.new_block("arguments_bundle.slow");
        let merge_idx = ctx.new_block("arguments_bundle.merge");
        let slow_label = ctx.block_label(slow_idx);
        let merge_label = ctx.block_label(merge_idx);
        let own_end = ctx.block().label.clone();
        ctx.block().cond_br(&not_own, &slow_label, &merge_label);

        ctx.current_block = slow_idx;
        let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        let (callee, wrapper) = if elided.restricted_callee {
            (undefined, "null".to_string())
        } else {
            match &elided.callee {
                ElidedArgumentsCallee::Undefined => (undefined, "null".to_string()),
                // The runtime materializes the singleton after rooting its
                // operands, so no allocation sits between `key` and the call.
                ElidedArgumentsCallee::FunctionWrapper(wrapper) => {
                    (undefined, format!("@{wrapper}"))
                }
                ElidedArgumentsCallee::CurrentClosure => {
                    let ptr = crate::expr::try_current_closure_ptr_value(ctx)
                        .unwrap_or_else(|| "%this_closure".to_string());
                    (nanbox_pointer_inline(ctx.block(), &ptr), "null".to_string())
                }
            }
        };
        let restricted = if elided.restricted_callee { "1" } else { "0" };
        let slow = ctx.block().call(
            DOUBLE,
            "js_arguments_bundle_get_slow",
            &[
                (DOUBLE, &raw_args),
                (DOUBLE, &key),
                (DOUBLE, &callee),
                (PTR, &wrapper),
                (I32, restricted),
            ],
        );
        let slow_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        Ok(Some(
            ctx.block()
                .phi(DOUBLE, &[(&own, &own_end), (&slow, &slow_end)]),
        ))
    })
}

/// Whether a method may expose the scalar-count direct-call clone.
///
/// This is deliberately stricter than the materialization elision above. A
/// user rest parameter still needs its own array, and a nested closure may
/// outlive the direct call, so both shapes remain on the public ABI even when
/// every syntactic use happens to be a `.length` read.
pub(crate) fn method_supports_arguments_length_direct_abi(method: &perry_hir::Function) -> bool {
    let Some(synth_param) = method
        .params
        .last()
        .filter(|p| p.arguments_object.is_some())
    else {
        return false;
    };
    if method
        .params
        .iter()
        .any(|p| p.is_rest && p.arguments_object.is_none())
    {
        return false;
    }
    let mut captured = false;
    crate::collectors::for_each_expr_in_stmts(&method.body, &mut |expr| {
        if let Expr::Closure { captures, .. } = expr {
            captured |= captures.contains(&synth_param.id);
        }
    });
    !captured && arguments_used_only_for_length(&method.body, synth_param.id)
}

fn mapped_arguments_params(params: &[Param]) -> Vec<(u32, u32)> {
    params
        .iter()
        .filter_map(|p| p.arguments_object.as_ref())
        .flat_map(|meta| meta.mapped_parameter_ids.iter().copied())
        .collect()
}

#[cfg(test)]
mod length_only_tests {
    use super::arguments_used_only_for_length;
    use perry_hir::{Expr, Stmt};

    const ARGUMENTS: u32 = 17;

    fn length() -> Expr {
        Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(ARGUMENTS)),
            property: "length".to_string(),
            byte_offset: 0,
        }
    }

    #[test]
    fn accepts_exact_length_reads_at_arbitrary_depth() {
        let body = vec![Stmt::Return(Some(Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left: Box::new(Expr::Integer(1)),
            right: Box::new(length()),
        }))];
        assert!(arguments_used_only_for_length(&body, ARGUMENTS));
    }

    #[test]
    fn rejects_identity_index_and_mixed_uses() {
        assert!(!arguments_used_only_for_length(
            &[Stmt::Return(Some(Expr::LocalGet(ARGUMENTS)))],
            ARGUMENTS
        ));
        assert!(!arguments_used_only_for_length(
            &[Stmt::Return(Some(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(ARGUMENTS)),
                index: Box::new(Expr::Integer(0)),
            }))],
            ARGUMENTS
        ));
        assert!(!arguments_used_only_for_length(
            &[
                Stmt::Expr(length()),
                Stmt::Return(Some(Expr::LocalGet(ARGUMENTS))),
            ],
            ARGUMENTS
        ));
    }

    #[test]
    fn rejects_specialized_local_bearing_operations() {
        let body = vec![
            Stmt::Expr(length()),
            Stmt::Return(Some(Expr::ArrayPop(ARGUMENTS))),
        ];
        assert!(!arguments_used_only_for_length(&body, ARGUMENTS));
    }
}

#[cfg(test)]
mod elision_tests {
    use super::{arguments_elision, ArgumentsElision};
    use perry_hir::types::Type;
    use perry_hir::{ArgumentsObjectMeta, Expr, Param, Stmt};

    const A: u32 = 3;
    const ARGUMENTS: u32 = 17;

    fn param(id: u32, name: &str) -> Param {
        Param {
            id,
            name: name.to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }
    }

    /// `(a, arguments)`; `mapped` makes it the sloppy simple-list shape whose
    /// object aliases `a`.
    fn params(mapped: bool) -> Vec<Param> {
        let mut synth = param(ARGUMENTS, "arguments");
        synth.is_rest = true;
        synth.arguments_object = Some(ArgumentsObjectMeta {
            strict: !mapped,
            simple_parameters: true,
            mapped_parameter_ids: if mapped { vec![(0, A)] } else { Vec::new() },
            restricted_callee: !mapped,
        });
        vec![param(A, "a"), synth]
    }

    fn args() -> Box<Expr> {
        Box::new(Expr::LocalGet(ARGUMENTS))
    }

    fn index(i: i64) -> Expr {
        Expr::IndexGet {
            object: args(),
            index: Box::new(Expr::Integer(i)),
        }
    }

    fn length() -> Expr {
        Expr::PropertyGet {
            object: args(),
            property: "length".to_string(),
            byte_offset: 0,
        }
    }

    fn reads() -> Vec<Stmt> {
        vec![
            Stmt::Expr(length()),
            Stmt::Return(Some(Expr::IndexGet {
                object: args(),
                index: Box::new(Expr::Binary {
                    op: perry_hir::BinaryOp::Sub,
                    left: Box::new(length()),
                    right: Box::new(index(0)),
                }),
            })),
        ]
    }

    #[test]
    fn length_and_index_reads_elide() {
        assert_eq!(
            arguments_elision(&params(false), &[Stmt::Return(Some(length()))]),
            Some(ArgumentsElision::LengthOnly)
        );
        assert_eq!(
            arguments_elision(&params(false), &reads()),
            Some(ArgumentsElision::Reads)
        );
    }

    #[test]
    fn reference_positions_keep_the_object() {
        // `delete arguments[0]`, `delete arguments.length`: the raw bundle has
        // different own-property attributes.
        for target in [index(0), length()] {
            let mut body = reads();
            body.push(Stmt::Expr(Expr::Delete(Box::new(target))));
            assert_eq!(arguments_elision(&params(false), &body), None);
        }
        // `arguments[0]()`: the object is the callee's `this`.
        let mut body = reads();
        body.push(Stmt::Expr(Expr::Call {
            callee: Box::new(index(0)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        }));
        assert_eq!(arguments_elision(&params(false), &body), None);
        // Identity escapes.
        let mut body = reads();
        body.push(Stmt::Expr(Expr::LocalGet(ARGUMENTS)));
        assert_eq!(arguments_elision(&params(false), &body), None);
    }

    #[test]
    fn a_parameter_default_is_part_of_the_proof() {
        let mut params = params(false);
        params[0].default = Some(Expr::LocalGet(ARGUMENTS));
        assert_eq!(arguments_elision(&params, &reads()), None);
        params[0].default = Some(index(1));
        assert_eq!(
            arguments_elision(&params, &reads()),
            Some(ArgumentsElision::Reads)
        );
    }

    #[test]
    fn a_mapped_parameter_must_keep_its_incoming_value() {
        let with = |extra: Stmt| {
            let mut body = vec![extra];
            body.extend(reads());
            arguments_elision(&params(true), &body)
        };
        assert_eq!(
            with(Stmt::Expr(Expr::LocalGet(A))),
            Some(ArgumentsElision::Reads)
        );
        assert_eq!(
            with(Stmt::Expr(Expr::LocalSet(A, Box::new(Expr::Integer(9))))),
            None
        );
        // `var a = 5` re-declaring the parameter reuses its id.
        assert_eq!(
            with(Stmt::Let {
                id: A,
                name: "a".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Integer(5)),
            }),
            None
        );
        // Length reads never observe the aliasing.
        assert_eq!(
            arguments_elision(
                &params(true),
                &[
                    Stmt::Expr(Expr::LocalSet(A, Box::new(Expr::Integer(9)))),
                    Stmt::Return(Some(length())),
                ]
            ),
            Some(ArgumentsElision::LengthOnly)
        );
    }
}

/// Does `property`, resolved against `class_name`'s ancestry, declare a USER
/// `...rest` parameter — as opposed to (or in addition to) the trailing
/// `arguments` slot #677 synthesizes?
///
/// #8040/#8162. Both spellings lower as `Param { is_rest: true }`, so
/// `method_has_rest` is true for either, and `method_has_synthetic_arguments`
/// only names the synthesized slot — the PAIR still cannot distinguish
/// "synthesized `arguments` only" from "user rest AND synthesized `arguments`",
/// and those fill a different number of trailing slots (`m(a, ...rest)` with an
/// `arguments` read is `[a, rest, arguments]`: TWO arrays, from two offsets).
/// The discriminator is `arguments_object`, which the synthesized parameter
/// carries and nothing else does.
///
/// Read off the class HIR, so a class the current module has no HIR for (an
/// imported class) reports `false`, leaving those call sites on the
/// one-trailing-slot behavior they had — `method_has_synthetic_arguments`
/// still covers the imported synth-only shape via its interface bit.
pub(crate) fn method_has_user_rest(
    ctx: &crate::expr::FnCtx<'_>,
    class_name: &str,
    property: &str,
) -> bool {
    let mut walk = Some(class_name.to_string());
    while let Some(cur) = walk {
        let class = ctx.classes.get(&cur);
        if let Some(f) = class.and_then(|c| c.methods.iter().find(|m| m.name == *property)) {
            return f
                .params
                .iter()
                .any(|p| p.is_rest && p.arguments_object.is_none());
        }
        walk = class.and_then(|c| c.extends_name.clone());
    }
    false
}
