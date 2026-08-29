//! Guarded loop versions for counted Array and Array-subclass iteration.
//!
//! Runtime admission publishes scalar layout facts. Ordinary bindings use a
//! call-free clone whose preheader-cached receiver stays valid throughout;
//! immutable closure captures reload and revalidate at every iteration. A
//! failed admission resumes the unchanged generic loop at the current counter.

use anyhow::Result;
use perry_hir::{CompareOp, Expr, Stmt, UpdateOp};

use crate::expr::{FnCtx, StablePackedLoopFact, StablePackedNumericAccess, StablePackedReadCache};
use crate::native_value::{BoundsState, BufferAccessMode, LoweredValue, MaterializationReason};
use crate::types::{DOUBLE, I1, I32, I64, PTR};

#[derive(Clone, Copy)]
enum LoopBound {
    Snapshot(u32),
    LiveLength,
}

struct Candidate {
    counter_id: u32,
    array_id: u32,
    bound: LoopBound,
    numeric_elements: bool,
    u32_index_elements: bool,
    capture_index: Option<u32>,
    capture_uses_box: bool,
    nested_derived: bool,
    nested_requires_access_revalidation: bool,
    cache_repeated_index_reads: bool,
}

fn required_numeric_mode(numeric_elements: bool, u32_index_elements: bool) -> &'static str {
    if u32_index_elements {
        "2"
    } else if numeric_elements {
        "1"
    } else {
        "0"
    }
}

fn exact_target_read(expr: &Expr, array_id: u32, counter_id: u32) -> bool {
    matches!(
        expr,
        Expr::IndexGet { object, index }
            if matches!(object.as_ref(), Expr::LocalGet(id) if *id == array_id)
                && matches!(index.as_ref(), Expr::LocalGet(id) if *id == counter_id)
    )
}

fn target_below_numeric_operator(
    expr: &Expr,
    array_id: u32,
    counter_id: u32,
    numeric_context: bool,
) -> bool {
    if exact_target_read(expr, array_id, counter_id) {
        return numeric_context;
    }
    if matches!(expr, Expr::Closure { .. }) {
        return false;
    }
    let child_numeric_context =
        numeric_context || matches!(expr, Expr::Binary { .. } | Expr::NumberCoerce(_));
    let mut found = false;
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        if !found
            && target_below_numeric_operator(child, array_id, counter_id, child_numeric_context)
        {
            found = true;
        }
    });
    found
}

/// Whether the admitted read is used as the complete key of another indexed
/// access. A guarded typed-array loop validates and canonicalizes this value
/// once per source iteration, then reuses the native `u32` at every component
/// access.
fn target_is_index_key(expr: &Expr, array_id: u32, counter_id: u32) -> bool {
    if matches!(expr, Expr::Closure { .. }) {
        return false;
    }
    let is_target = |candidate: &Expr| exact_target_read(candidate, array_id, counter_id);
    match expr {
        Expr::IndexGet { object, index }
        | Expr::IndexSet { object, index, .. }
        | Expr::IndexUpdate { object, index, .. } => {
            if is_target(index) {
                return true;
            }
            target_is_index_key(object, array_id, counter_id)
                || target_is_index_key(index, array_id, counter_id)
        }
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            if is_target(key) {
                return true;
            }
            [
                target.as_ref(),
                key.as_ref(),
                value.as_ref(),
                receiver.as_ref(),
            ]
            .into_iter()
            .any(|child| target_is_index_key(child, array_id, counter_id))
        }
        _ => {
            let mut found = false;
            perry_hir::walker::walk_expr_children(expr, &mut |child| {
                if !found && target_is_index_key(child, array_id, counter_id) {
                    found = true;
                }
            });
            found
        }
    }
}

fn leading_read_requires_numeric(body: &[Stmt], array_id: u32, counter_id: u32) -> bool {
    let Some(first) = body.first() else {
        return false;
    };
    let expr = match first {
        Stmt::Let {
            init: Some(expr), ..
        }
        | Stmt::Expr(expr)
        | Stmt::Throw(expr)
        | Stmt::Return(Some(expr)) => expr,
        _ => return false,
    };
    target_below_numeric_operator(expr, array_id, counter_id, false)
}

fn leading_read_requires_u32_index(body: &[Stmt], array_id: u32, counter_id: u32) -> bool {
    let Some(first) = body.first() else {
        return false;
    };
    let expr = match first {
        Stmt::Let {
            init: Some(expr), ..
        }
        | Stmt::Expr(expr)
        | Stmt::Throw(expr)
        | Stmt::Return(Some(expr)) => expr,
        _ => return false,
    };
    target_is_index_key(expr, array_id, counter_id)
}

fn expr_flags(expr: &Expr, array_id: u32, counter_id: u32, target: &mut bool, call: &mut bool) {
    if matches!(
        expr,
        Expr::IndexGet { object, index }
            if matches!(object.as_ref(), Expr::LocalGet(id) if *id == array_id)
                && matches!(index.as_ref(), Expr::LocalGet(id) if *id == counter_id)
    ) {
        *target = true;
    }
    if matches!(expr, Expr::Call { .. } | Expr::New { .. }) {
        *call = true;
    }
    if !matches!(expr, Expr::Closure { .. }) {
        perry_hir::walker::walk_expr_children(expr, &mut |child| {
            expr_flags(child, array_id, counter_id, target, call);
        });
    }
}

fn stmt_flags(stmt: &Stmt, array_id: u32, counter_id: u32) -> (bool, bool) {
    let mut target = false;
    let mut call = false;
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        }
        | Stmt::Expr(expr)
        | Stmt::Throw(expr)
        | Stmt::Return(Some(expr)) => {
            expr_flags(expr, array_id, counter_id, &mut target, &mut call);
        }
        _ => {}
    }
    (target, call)
}

/// Count exact `receiver[counter]` reads without descending into closures.
fn target_read_count(expr: &Expr, array_id: u32, counter_id: u32) -> usize {
    if matches!(
        expr,
        Expr::IndexGet { object, index }
            if matches!(object.as_ref(), Expr::LocalGet(id) if *id == array_id)
                && matches!(index.as_ref(), Expr::LocalGet(id) if *id == counter_id)
    ) {
        return 1;
    }
    if matches!(expr, Expr::Closure { .. }) {
        return 0;
    }
    let mut count = 0;
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        count += target_read_count(child, array_id, counter_id);
    });
    count
}

/// Count exact target reads in the straight-line statements admitted here.
fn body_target_read_count(body: &[Stmt], array_id: u32, counter_id: u32) -> usize {
    body.iter()
        .map(|stmt| match stmt {
            Stmt::Let {
                init: Some(expr), ..
            }
            | Stmt::Expr(expr)
            | Stmt::Throw(expr)
            | Stmt::Return(Some(expr)) => target_read_count(expr, array_id, counter_id),
            _ => 0,
        })
        .sum()
}

/// A repeated element value can survive calls only through the dirty-bit
/// protocol below. Direct writes need a separate alias argument. A statically
/// proven TypedArray store is brand-disjoint from the admitted
/// Array/Array-subclass receiver. An erased IndexSet has the same property on
/// its sole no-call arm; every other brand crosses a dirtying runtime call.
/// Property writes, statically Array stores, and in-place Array operations
/// disable value caching.
fn indexed_store_direct_arm_is_brand_disjoint(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    matches!(
        crate::type_analysis::static_type_of(ctx, object),
        None | Some(perry_hir::types::Type::Any) | Some(perry_hir::types::Type::Unknown)
    ) || crate::type_analysis::is_typed_array_expr(ctx, object)
}

/// Whether `expr` has a direct mutation arm that can alias the cached Array.
fn expr_blocks_repeated_read_cache(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    if matches!(
        expr,
        Expr::PropertySet { .. }
            | Expr::PropertyUpdate { .. }
            | Expr::SuperPropertySet { .. }
            | Expr::ObjectSuperPropertySet { .. }
            | Expr::ObjectAssign { .. }
            | Expr::ObjectDefineProperty(..)
            | Expr::ObjectDefineProperties(..)
            | Expr::ObjectSetPrototypeOf(..)
            | Expr::ArrayPush { .. }
            | Expr::ArrayPushSpread { .. }
            | Expr::ArrayPop(..)
            | Expr::ArrayShift(..)
            | Expr::ArrayUnshift { .. }
            | Expr::ArraySplice { .. }
            | Expr::ArraySort { .. }
            | Expr::ArrayReverseValue { .. }
            | Expr::ArrayCopyWithin { .. }
            | Expr::ArrayCopyWithinValue { .. }
    ) {
        return true;
    }
    if let Expr::IndexSet { object, .. } = expr {
        // An erased IndexSet's only no-call direct arm is the guarded
        // TypedArray store; every other brand reaches `js_dyn_index_set`,
        // which dirties the proof before mutating. This is exactly Wolf's
        // unannotated component-column shape. A statically Array-typed store,
        // on the other hand, can directly mutate an alias of the source.
        if !indexed_store_direct_arm_is_brand_disjoint(ctx, object) {
            return true;
        }
    }
    if let Expr::PutValueSet {
        target,
        key,
        receiver,
        ..
    } = expr
    {
        // Source assignments reach HIR as PutValueSet. The codegen's narrow
        // same-receiver, non-string-key route immediately delegates to the
        // IndexSet arm described above. Match only the side-effect-free local
        // identity form here; every explicit-receiver or computed-base form
        // remains conservatively blocked.
        let same_local = matches!(
            (target.as_ref(), receiver.as_ref()),
            (Expr::LocalGet(target_id), Expr::LocalGet(receiver_id))
                if target_id == receiver_id
        );
        let static_string_or_symbol = matches!(
            key.as_ref(),
            Expr::String(_) | Expr::WtfString(_) | Expr::SymbolFor(_)
        ) || crate::type_analysis::is_string_expr(ctx, key);
        if !same_local
            || static_string_or_symbol
            || !indexed_store_direct_arm_is_brand_disjoint(ctx, target)
        {
            return true;
        }
    }
    if let Expr::IndexUpdate { object, .. } = expr {
        // The update lowering has more direct receiver arms than IndexSet, so
        // require a static TypedArray brand rather than admitting `any`.
        if !crate::type_analysis::is_typed_array_expr(ctx, object) {
            return true;
        }
    }
    if matches!(expr, Expr::Closure { .. }) {
        return false;
    }
    let mut blocked = false;
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        if !blocked && expr_blocks_repeated_read_cache(ctx, child) {
            blocked = true;
        }
    });
    blocked
}

/// Whether any admitted body statement can directly invalidate the cache.
fn body_blocks_repeated_read_cache(ctx: &FnCtx<'_>, body: &[Stmt]) -> bool {
    body.iter().any(|stmt| match stmt {
        Stmt::Let {
            init: Some(expr), ..
        }
        | Stmt::Expr(expr)
        | Stmt::Throw(expr)
        | Stmt::Return(Some(expr)) => expr_blocks_repeated_read_cache(ctx, expr),
        _ => false,
    })
}

/// The direct read must be in the first straight-line statement and before any
/// explicit user call. Later statements may allocate or invoke callbacks: the
/// next iteration reloads the root and validates before using it again.
fn body_has_safe_leading_read(
    body: &[Stmt],
    array_id: u32,
    counter_id: u32,
    allow_revalidated_later_reads: bool,
) -> bool {
    for (index, stmt) in body.iter().enumerate() {
        let (has_target, has_call) = stmt_flags(stmt, array_id, counter_id);
        if has_target {
            return !has_call
                && (allow_revalidated_later_reads
                    || !body[index + 1..]
                        .iter()
                        .any(|later| stmt_flags(later, array_id, counter_id).0));
        }
        // Compound indexed assignments are lowered into pure receiver/key
        // temporaries before the source indexed read. Replaying these local
        // copies on a side exit has no observable effect. Keep the admitted
        // prefix deliberately narrow; property reads, calls, and writes stay
        // generic.
        if !matches!(
            stmt,
            Stmt::Let {
                init: Some(Expr::LocalGet(_)),
                ..
            }
        ) {
            return false;
        }
    }
    false
}

fn stmt_contains_break(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Break | Stmt::LabeledBreak(_) => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.iter().any(stmt_contains_break)
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| branch.iter().any(stmt_contains_break))
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            body.iter().any(stmt_contains_break)
        }
        Stmt::For { init, body, .. } => {
            init.as_deref().is_some_and(stmt_contains_break) || body.iter().any(stmt_contains_break)
        }
        Stmt::Labeled { body, .. } => stmt_contains_break(body),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(stmt_contains_break)
                || catch
                    .as_ref()
                    .is_some_and(|clause| clause.body.iter().any(stmt_contains_break))
                || finally
                    .as_ref()
                    .is_some_and(|body| body.iter().any(stmt_contains_break))
        }
        Stmt::Switch { cases, .. } => cases
            .iter()
            .any(|case| case.body.iter().any(stmt_contains_break)),
        _ => false,
    }
}

fn record_capture_rejection(ctx: &mut FnCtx<'_>, array_id: u32, reason: &str) {
    let lowered = LoweredValue::js_value("closure_capture_candidate".to_string());
    ctx.record_lowered_value_with_access_mode_and_facts(
        "StablePackedArraylikeLoop",
        Some(array_id),
        "stable_packed_arraylike_capture_rejected",
        &lowered,
        Some(BoundsState::Unknown),
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(MaterializationReason::RuntimeApi),
        None,
        None,
        Vec::new(),
        Vec::new(),
        false,
        false,
        vec![
            "candidate_storage=closure_capture_slot".to_string(),
            format!("rejection={reason}"),
            "fallback=generic_counted_loop".to_string(),
        ],
    );
}

fn match_candidate(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Option<Candidate> {
    if !ctx.pending_labels.is_empty() {
        return None;
    }
    let counter_id = match init? {
        Stmt::Let {
            id,
            init: Some(Expr::Integer(0)),
            ..
        } => *id,
        _ => return None,
    };
    if !matches!(
        update,
        Some(Expr::Update {
            id,
            op: UpdateOp::Increment,
            ..
        }) if *id == counter_id
    ) {
        return None;
    }
    let right = match condition? {
        Expr::Compare {
            op: CompareOp::Lt,
            left,
            right,
        } if matches!(left.as_ref(), Expr::LocalGet(id) if *id == counter_id) => right.as_ref(),
        _ => return None,
    };
    let (array_id, bound) = match right {
        Expr::LocalGet(bound_id) => {
            let array_id = *ctx.array_length_snapshots.get(bound_id)?;
            if ctx.reassigned_locals.contains(bound_id) {
                return None;
            }
            (array_id, LoopBound::Snapshot(*bound_id))
        }
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => match object.as_ref() {
            Expr::LocalGet(array_id) => (*array_id, LoopBound::LiveLength),
            _ => return None,
        },
        _ => return None,
    };
    let receiver = Expr::LocalGet(array_id);
    let capture_index = ctx.closure_captures.get(&array_id).copied();
    let derived_parent = ctx
        .stable_packed_loop_facts
        .iter()
        .rev()
        .find(|fact| fact.derived_locals.contains(&array_id));
    let nested_derived = derived_parent.is_some();
    let nested_requires_access_revalidation = derived_parent
        .is_some_and(|fact| fact.revalidate_each_iteration || fact.revalidate_before_indexed_read);
    let cache_repeated_index_reads = nested_requires_access_revalidation
        && body_target_read_count(body, array_id, counter_id) > 1
        && !body_blocks_repeated_read_cache(ctx, body);
    let storage_is_available = capture_index.is_some()
        || (ctx.locals.contains_key(&array_id) && !ctx.boxed_vars.contains(&array_id))
        || (!ctx.locals.contains_key(&array_id) && ctx.module_globals.contains_key(&array_id));
    let binding_is_eligible = if capture_index.is_some() || nested_derived {
        // Capturing the binding is itself an identity exposure in the
        // whole-function fact graph. That historical hazard is exactly what
        // this version repairs: an immutable capture is reloaded and fully
        // guarded at every iteration, so an alias may mutate the object only
        // by making the next guard fail to the generic loop. Semantic
        // rebinding remains represented in `reassigned_locals` and is rejected
        // below; a compiler-only TDZ/hoisting box does not imply mutation.
        !ctx.scalar_replaced_arrays.contains_key(&array_id)
    } else {
        super::loops::packed_loop_array_binding_is_eligible(ctx, array_id)
    };
    let leading_read_is_first = body.first().is_some_and(|stmt| {
        let (has_target, has_call) = stmt_flags(stmt, array_id, counter_id);
        has_target && !has_call
    });
    let rejection = if ctx.reassigned_locals.contains(&array_id) {
        Some("reassigned_binding")
    } else if !storage_is_available {
        Some("unavailable_storage")
    // TypedArrays have their own element-width-aware indexed lowering. Even
    // though the runtime guard would decline their non-Array header, emitting
    // the speculative clone can feed its numeric facts into function-wide
    // native-representation selection.
    } else if crate::type_analysis::is_typed_array_expr(ctx, &receiver) {
        Some("known_typed_array")
    } else if super::loops::stmts_mutate_local(body, counter_id) {
        Some("counter_mutated_in_body")
    // A fast-loop `break` reaches that clone's exit block. Live-length
    // versions use the same block to enter the generic continuation, so
    // replaying the current iteration would duplicate preceding effects.
    } else if body.iter().any(stmt_contains_break) {
        Some("break_replays_current_iteration")
    } else if !body_has_safe_leading_read(
        body,
        array_id,
        counter_id,
        nested_requires_access_revalidation,
    ) {
        Some("indexed_read_not_safe_and_leading")
    // LocalGet prefixes are replay-safe for a nested derived receiver, whose
    // guard is emitted at the indexed read. A capture is guarded at iteration
    // entry instead, and another captured LocalGet in such a prefix can run a
    // GC helper before the cached address is consumed. Keep that shape on the
    // generic path until entry guards can be placed after the prefix.
    } else if capture_index.is_some() && !leading_read_is_first {
        Some("capture_read_after_safepoint_capable_prefix")
    // Preserve the existing escape/materialization contract for ordinary
    // locals/globals. Captures use their separate guarded eligibility above.
    } else if !binding_is_eligible {
        Some("binding_not_eligible")
    } else {
        None
    };
    if let Some(reason) = rejection {
        if capture_index.is_some() {
            record_capture_rejection(ctx, array_id, reason);
        }
        return None;
    }
    let u32_index_elements = leading_read_requires_u32_index(body, array_id, counter_id);
    Some(Candidate {
        counter_id,
        array_id,
        bound,
        numeric_elements: u32_index_elements
            || leading_read_requires_numeric(body, array_id, counter_id),
        u32_index_elements,
        capture_index,
        capture_uses_box: capture_index.is_some() && ctx.boxed_vars.contains(&array_id),
        nested_derived,
        nested_requires_access_revalidation,
        cache_repeated_index_reads,
    })
}

/// Mark a local whose initializer is the exact direct indexed read admitted by
/// the active stable-packed fact. The mark lives on that fact, so it cannot
/// leak from the fast clone into the generic clone.
pub(super) fn record_derived_local(ctx: &mut FnCtx<'_>, id: u32, init: &Expr, mutable: bool) {
    if mutable || ctx.reassigned_locals.contains(&id) {
        return;
    }
    if crate::expr::is_proven_u32_view_read(ctx, init) {
        let native_slot = ctx.func.alloca_entry(I32);
        if let Some(fact) = ctx.stable_packed_loop_facts.last_mut() {
            fact.u32_view_derived_locals.insert(id, native_slot);
        }
    }
    let Expr::IndexGet { object, index } = init else {
        return;
    };
    let (Expr::LocalGet(array_id), Expr::LocalGet(index_id)) = (object.as_ref(), index.as_ref())
    else {
        return;
    };
    let Some(fact) = ctx
        .stable_packed_loop_facts
        .iter_mut()
        .rev()
        .find(|fact| fact.array_local_id == *array_id && fact.counter_local_id == *index_id)
    else {
        return;
    };
    fact.derived_locals.insert(id);
}

pub(crate) fn u32_view_derived_local_slot(ctx: &FnCtx<'_>, id: u32) -> Option<String> {
    ctx.stable_packed_loop_facts
        .iter()
        .rev()
        .find_map(|fact| fact.u32_view_derived_locals.get(&id).cloned())
}

fn descriptor_word(ctx: &mut FnCtx<'_>, descriptor: &str, index: u64) -> String {
    let ptr = ctx
        .block()
        .gep(I64, descriptor, &[(I64, &index.to_string())]);
    ctx.block().load(I64, &ptr)
}

/// Derive raw numeric storage bases from a freshly validated receiver. This is
/// used once in ordinary call-free loops and at every iteration entry for a
/// closure capture, where a nested guard/callback may have moved the receiver
/// since the preceding iteration.
/// The kind test and payload base shared by the two plain-payload kinds.
///
/// Kind 1 means the receiver IS the `ArrayHeader`, so its payload starts at
/// `live_raw + 8`. Kind 3 is an elements-backed Array subclass
/// (`perry-runtime::array::subclass_elements`): the receiver is the object and
/// the payload lives in a separate Array whose address the guard publishes in
/// descriptor word 3 and every revalidation refreshes. Selecting the base from
/// that word keeps both the capture-safe path (which reloads the receiver) and
/// the ordinary path (whose `live_raw` is the receiver) reading the payload.
fn plain_payload_base(ctx: &mut FnCtx<'_>, descriptor: &str, live_raw: &str) -> (String, String) {
    let kind = descriptor_word(ctx, descriptor, 0);
    let is_array_receiver = ctx.block().icmp_eq(I64, &kind, "1");
    let is_elements_store = ctx.block().icmp_eq(I64, &kind, "3");
    let is_plain = ctx.block().or(I1, &is_array_receiver, &is_elements_store);
    let store = descriptor_word(ctx, descriptor, 3);
    let payload = ctx
        .block()
        .select(I1, &is_elements_store, I64, &store, live_raw);
    (is_plain, payload)
}

fn build_numeric_access(
    ctx: &mut FnCtx<'_>,
    descriptor: &str,
    live_raw: &str,
    contiguous_u32_prefix: bool,
) -> StablePackedNumericAccess {
    let (is_plain, payload) = plain_payload_base(ctx, descriptor, live_raw);
    let plain_base = ctx.block().add(I64, &payload, "8");

    let element_base = descriptor_word(ctx, descriptor, 4);
    let packed_bounds = descriptor_word(ctx, descriptor, 5);
    let inline_bound = ctx.block().lshr(I64, &packed_bounds, "32");
    let has_inline = ctx.block().icmp_ult(I64, &element_base, &inline_bound);
    let inline_span = ctx.block().sub(I64, &inline_bound, &element_base);
    let object_inline_count = ctx.block().select(I1, &has_inline, I64, &inline_span, "0");
    let element_bytes = ctx.block().shl(I64, &element_base, "3");
    let object_header_size =
        crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let inline_offset = ctx.block().add(I64, &object_header_size, &element_bytes);
    let object_inline_base = ctx.block().add(I64, live_raw, &inline_offset);

    // Only Array-subclass objects own ObjectMeta. Keep the metadata load
    // control-dependent so a plain Array never interprets element bits as a
    // pointer. A missing spill is valid when the admitted bound fits inline.
    let plain_setup_idx = ctx.new_block("stable_packed.setup.plain");
    let object_setup_idx = ctx.new_block("stable_packed.setup.object");
    let meta_setup_idx = ctx.new_block("stable_packed.setup.meta");
    let setup_merge_idx = ctx.new_block("stable_packed.setup.merge");
    let plain_setup_label = ctx.block_label(plain_setup_idx);
    let object_setup_label = ctx.block_label(object_setup_idx);
    let meta_setup_label = ctx.block_label(meta_setup_idx);
    let setup_merge_label = ctx.block_label(setup_merge_idx);
    ctx.block()
        .cond_br(&is_plain, &plain_setup_label, &object_setup_label);

    ctx.current_block = plain_setup_idx;
    ctx.block().br(&setup_merge_label);

    ctx.current_block = object_setup_idx;
    let pointer_size = if crate::target_layout::target_is_ilp32(ctx.target_triple) {
        4
    } else {
        8
    };
    let meta_offset =
        crate::target_layout::object_meta_slot_offset_bytes(ctx.target_triple).to_string();
    let meta_addr = ctx.block().add(I64, live_raw, &meta_offset);
    let meta_slot = ctx.block().inttoptr(I64, &meta_addr);
    let meta_native = ctx
        .block()
        .load(if pointer_size == 4 { I32 } else { I64 }, &meta_slot);
    let meta = if pointer_size == 4 {
        ctx.block().zext(I32, &meta_native, I64)
    } else {
        meta_native
    };
    let has_meta = ctx.block().icmp_ne(I64, &meta, "0");
    ctx.block()
        .cond_br(&has_meta, &meta_setup_label, &setup_merge_label);

    ctx.current_block = meta_setup_idx;
    let meta_ptr = ctx.block().inttoptr(I64, &meta);
    let spill_slot = ctx.block().gep(I64, &meta_ptr, &[(I64, "4")]);
    let spill = ctx.block().load(I64, &spill_slot);
    ctx.block().br(&setup_merge_label);

    ctx.current_block = setup_merge_idx;
    let spill = ctx.block().phi(
        I64,
        &[
            ("0", &plain_setup_label),
            ("0", &object_setup_label),
            (&spill, &meta_setup_label),
        ],
    );
    let has_spill = ctx.block().icmp_ne(I64, &spill, "0");
    let safe_spill = ctx.block().select(I1, &has_spill, I64, &spill, live_raw);
    let spill_offset = ctx.block().add(I64, &element_bytes, "8");
    let object_spill_base = ctx.block().add(I64, &safe_spill, &spill_offset);
    let contiguous_base = contiguous_u32_prefix.then(|| {
        // Mode-2 admission rejects prefixes that cross the inline/spill
        // boundary (and rejects plain Arrays), so storage selection belongs
        // in this preheader rather than in every entity iteration.
        ctx.block().select(
            I1,
            &has_inline,
            I64,
            &object_inline_base,
            &object_spill_base,
        )
    });
    StablePackedNumericAccess {
        contiguous_base,
        is_plain,
        plain_base,
        object_inline_count,
        object_inline_base,
        object_spill_base,
    }
}

fn record_artifacts(ctx: &mut FnCtx<'_>, candidate: &Candidate, receiver: &str) {
    let array_id = candidate.array_id;
    let lowered = LoweredValue::js_value(receiver.to_string());
    let mut selected_facts = vec![
        "loop_versioning=stable_packed_arraylike".to_string(),
        "proof=preheader_scalar_layout".to_string(),
        if candidate.capture_index.is_some() {
            "candidate_storage=closure_capture_slot".to_string()
        } else {
            "candidate_storage=addressable_binding".to_string()
        },
        if candidate.capture_index.is_some() {
            "revalidation=each_iteration_capture_reload".to_string()
        } else if candidate.nested_requires_access_revalidation {
            "revalidation=before_nested_indexed_read".to_string()
        } else {
            "revalidation=none_call_free_clone".to_string()
        },
        format!(
            "guard_identity=stable_packed_arraylike:{}:{}",
            candidate.array_id, candidate.counter_id
        ),
        "side_exit=current_index".to_string(),
    ];
    if let Some(capture_index) = candidate.capture_index {
        selected_facts.push(format!("capture_index={capture_index}"));
        selected_facts.push(format!(
            "capture_value_storage={}",
            if candidate.capture_uses_box {
                "compiler_box"
            } else {
                "inline_value"
            }
        ));
    }
    if candidate.nested_derived {
        selected_facts.push("candidate_origin=guarded_outer_index_read".to_string());
        if candidate.nested_requires_access_revalidation {
            selected_facts
                .push("nested_read_miss=generic_read_without_iteration_replay".to_string());
        }
    }
    if candidate.cache_repeated_index_reads {
        selected_facts.push("same_counter_read_cache=call_invalidated".to_string());
    }
    ctx.record_lowered_value_with_access_mode_and_facts(
        "StablePackedArraylikeLoop",
        Some(array_id),
        "stable_packed_arraylike_preheader",
        &lowered,
        Some(BoundsState::Guarded {
            guard_id: "packed_arraylike_loop_guard".to_string(),
        }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
        false,
        false,
        selected_facts,
    );
    ctx.record_lowered_value_with_access_mode_and_facts(
        "StablePackedArraylikeLoop",
        Some(array_id),
        "stable_packed_arraylike_generic_side_exit",
        &lowered,
        Some(BoundsState::Unknown),
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(MaterializationReason::RuntimeApi),
        None,
        None,
        Vec::new(),
        Vec::new(),
        false,
        false,
        vec![
            "loop_versioning=stable_packed_arraylike_fallback".to_string(),
            format!(
                "fallback_identity=stable_packed_arraylike:{}:{}",
                candidate.array_id, candidate.counter_id
            ),
            "resume=current_index".to_string(),
        ],
    );
}

pub(crate) fn try_lower_index_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Option<String> {
    let (Expr::LocalGet(array_id), Expr::LocalGet(counter_id)) = (object, index) else {
        return None;
    };
    let fact = ctx
        .stable_packed_loop_facts
        .iter()
        .rev()
        .find(|fact| fact.array_local_id == *array_id && fact.counter_local_id == *counter_id)?
        .clone();
    // Load the scalar loop index before a nested-derived guard branches. Both
    // the direct arm and its exact-source generic fallback consume this value,
    // so it must dominate both successors.
    let counter_slot = ctx.i32_counter_slots.get(counter_id)?.clone();
    let idx_i32 = ctx.block().load(I32, &counter_slot);
    let repeated_read_cache = begin_repeated_read_cache(ctx, &fact, &idx_i32);
    let mut per_read_fallback = None;
    let mut per_read_live_raw = None;
    let mut per_read_numeric_access = None;
    if fact.revalidate_before_indexed_read {
        let dirty_slot = fact.revalidation_dirty_slot.as_ref()?;
        let live_raw_slot = fact.revalidation_live_raw_slot.as_ref()?;
        let receiver_slot = ctx.locals.get(array_id)?.clone();
        let receiver = ctx.block().load(DOUBLE, &receiver_slot);
        let dirty = ctx.block().load(I1, dirty_slot);
        let validate_idx = ctx.new_block("stable_packed.indexed_read.proof_dirty");
        let clean_idx = ctx.new_block("stable_packed.indexed_read.proof_clean");
        let live_merge_idx = ctx.new_block("stable_packed.indexed_read.live_merge");
        let validate_label = ctx.block_label(validate_idx);
        let clean_label = ctx.block_label(clean_idx);
        let live_merge_label = ctx.block_label(live_merge_idx);
        ctx.block().cond_br(&dirty, &validate_label, &clean_label);

        ctx.current_block = clean_idx;
        let clean_raw = ctx.block().load(I64, live_raw_slot);
        let clean_end = ctx.block().label.clone();
        ctx.block().br(&live_merge_label);

        ctx.current_block = validate_idx;
        let live_raw = ctx.block().call(
            I64,
            "js_packed_arraylike_loop_revalidate_live",
            &[
                (DOUBLE, &receiver),
                (DOUBLE, &fact.bound),
                (
                    I32,
                    required_numeric_mode(fact.numeric_elements, fact.u32_index_elements),
                ),
                (PTR, &fact.descriptor),
            ],
        );
        let mut pass = ctx.block().icmp_ne(I64, &live_raw, "0");
        if fact.live_length_bound {
            let refreshed_bound = descriptor_word(ctx, &fact.descriptor, 6);
            let length_unchanged = ctx
                .block()
                .icmp_eq(I64, &refreshed_bound, &fact.admitted_bound);
            pass = ctx.block().and(I1, &pass, &length_unchanged);
        }
        let continue_idx = ctx.new_block("stable_packed.indexed_read.derived_valid");
        // A later occurrence of `array[counter]` may follow an observable
        // getter, proxy trap, or store in the same source iteration. A failed
        // revalidation therefore cannot side-exit to the generic loop at the
        // current counter: that would replay the earlier effects. Fall back
        // for this one indexed read and merge back at the exact source point.
        let continue_label = ctx.block_label(continue_idx);
        let fallback = (!fact.u32_index_elements).then(|| {
            let fallback_idx = ctx.new_block("packed_index.generic_fallback");
            let read_merge_idx = ctx.new_block("packed_index.revalidated_merge");
            let fallback_label = ctx.block_label(fallback_idx);
            (fallback_idx, read_merge_idx, fallback_label)
        });
        let miss_label = fallback
            .as_ref()
            .map(|(_, _, label)| label.as_str())
            .unwrap_or(fact.side_exit_label.as_str());
        ctx.block().cond_br(&pass, &continue_label, miss_label);
        ctx.current_block = continue_idx;
        ctx.block().store(I64, &live_raw, live_raw_slot);
        ctx.block().store(I1, "0", dirty_slot);
        let validated_end = ctx.block().label.clone();
        ctx.block().br(&live_merge_label);

        ctx.current_block = live_merge_idx;
        let merged_live_raw = ctx.block().phi(
            I64,
            &[(&clean_raw, &clean_end), (&live_raw, &validated_end)],
        );
        per_read_numeric_access = fact.numeric_elements.then(|| {
            build_numeric_access(
                ctx,
                &fact.descriptor,
                &merged_live_raw,
                fact.u32_index_elements,
            )
        });
        per_read_live_raw = Some(merged_live_raw);
        if let Some((fallback_idx, read_merge_idx, fallback_label)) = fallback {
            per_read_fallback = Some((fallback_idx, read_merge_idx, fallback_label, receiver));
        }
    }
    let fact = ctx
        .stable_packed_loop_facts
        .iter()
        .rev()
        .find(|fact| fact.array_local_id == *array_id && fact.counter_local_id == *counter_id)?
        .clone();
    let u32_oob_label = u32_out_of_bounds_label(&fact).to_string();
    let raw = per_read_live_raw.or(fact.live_receiver_handle)?;
    let idx_i64 = ctx.block().zext(I32, &idx_i32, I64);
    if let Some(access) = per_read_numeric_access.or(fact.numeric_access) {
        let byte_offset = ctx.block().shl(I64, &idx_i64, "3");
        if let Some(base) = access.contiguous_base.as_ref() {
            let element_addr = ctx.block().add(I64, base, &byte_offset);
            let element_ptr = ctx.block().inttoptr(I64, &element_addr);
            let (direct, native_u32) = if fact.u32_index_elements {
                let native = ctx.block().load(I32, &element_ptr);
                (ctx.block().uitofp(I32, &native, DOUBLE), Some(native))
            } else {
                (ctx.block().load(DOUBLE, &element_ptr), None)
            };
            let resolved = finish_revalidated_read(ctx, direct, idx_i32.clone(), per_read_fallback);
            return Some(finish_repeated_read_cache(
                ctx,
                resolved,
                idx_i32,
                repeated_read_cache,
                native_u32.as_deref(),
                &u32_oob_label,
                fact.u32_component_bound.as_deref(),
            ));
        }
        let plain_addr = ctx.block().add(I64, &access.plain_base, &byte_offset);
        let inline_addr = ctx
            .block()
            .add(I64, &access.object_inline_base, &byte_offset);
        let spill_addr = ctx
            .block()
            .add(I64, &access.object_spill_base, &byte_offset);
        let is_inline = ctx
            .block()
            .icmp_ult(I64, &idx_i64, &access.object_inline_count);
        let object_addr = ctx
            .block()
            .select(I1, &is_inline, I64, &inline_addr, &spill_addr);
        let element_addr = ctx
            .block()
            .select(I1, &access.is_plain, I64, &plain_addr, &object_addr);
        let element_ptr = ctx.block().inttoptr(I64, &element_addr);
        let direct = ctx.block().load(DOUBLE, &element_ptr);
        let resolved = finish_revalidated_read(ctx, direct, idx_i32.clone(), per_read_fallback);
        return Some(finish_repeated_read_cache(
            ctx,
            resolved,
            idx_i32,
            repeated_read_cache,
            None,
            &u32_oob_label,
            fact.u32_component_bound.as_deref(),
        ));
    }
    let (is_plain, payload) = plain_payload_base(ctx, &fact.descriptor, &raw);

    let plain_idx = ctx.new_block("stable_packed.load.plain");
    let object_idx = ctx.new_block("stable_packed.load.object");
    let object_inline_idx = ctx.new_block("stable_packed.load.object.inline");
    let object_spill_idx = ctx.new_block("stable_packed.load.object.spill");
    let object_spill_ptr_idx = ctx.new_block("stable_packed.load.object.spill_ptr");
    let merge_idx = ctx.new_block("stable_packed.load.merge");
    let plain_label = ctx.block_label(plain_idx);
    let object_label = ctx.block_label(object_idx);
    let object_inline_label = ctx.block_label(object_inline_idx);
    let object_spill_label = ctx.block_label(object_spill_idx);
    let object_spill_ptr_label = ctx.block_label(object_spill_ptr_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&is_plain, &plain_label, &object_label);

    ctx.current_block = plain_idx;
    let byte_offset = ctx.block().shl(I64, &idx_i64, "3");
    let with_header = ctx.block().add(I64, &byte_offset, "8");
    let element_addr = ctx.block().add(I64, &payload, &with_header);
    let element_ptr = ctx.block().inttoptr(I64, &element_addr);
    let plain_raw = ctx.block().load(DOUBLE, &element_ptr);
    let plain_bits = ctx.block().bitcast_double_to_i64(&plain_raw);
    let is_hole = ctx
        .block()
        .icmp_eq(I64, &plain_bits, crate::nanbox::TAG_HOLE_I64);
    let undefined = ctx
        .block()
        .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
    let plain_value = ctx
        .block()
        .select(I1, &is_hole, DOUBLE, &undefined, &plain_raw);
    let plain_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = object_idx;
    let element_base = descriptor_word(ctx, &fact.descriptor, 4);
    let packed_bounds = descriptor_word(ctx, &fact.descriptor, 5);
    let inline_bound = ctx.block().lshr(I64, &packed_bounds, "32");
    let slot = ctx.block().add(I64, &element_base, &idx_i64);
    let inline = ctx.block().icmp_ult(I64, &slot, &inline_bound);
    ctx.block()
        .cond_br(&inline, &object_inline_label, &object_spill_label);

    ctx.current_block = object_inline_idx;
    let object_header_size =
        crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let slot_bytes = ctx.block().shl(I64, &slot, "3");
    let slot_offset = ctx.block().add(I64, &slot_bytes, &object_header_size);
    let slot_addr = ctx.block().add(I64, &raw, &slot_offset);
    let slot_ptr = ctx.block().inttoptr(I64, &slot_addr);
    let inline_value = ctx.block().load(DOUBLE, &slot_ptr);
    let inline_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = object_spill_idx;
    let pointer_size = if crate::target_layout::target_is_ilp32(ctx.target_triple) {
        4
    } else {
        8
    };
    let meta_offset =
        crate::target_layout::object_meta_slot_offset_bytes(ctx.target_triple).to_string();
    let meta_addr = ctx.block().add(I64, &raw, &meta_offset);
    let meta_slot = ctx.block().inttoptr(I64, &meta_addr);
    let meta_native = ctx
        .block()
        .load(if pointer_size == 4 { I32 } else { I64 }, &meta_slot);
    let meta = if pointer_size == 4 {
        ctx.block().zext(I32, &meta_native, I64)
    } else {
        meta_native
    };
    let read_miss_label = per_read_fallback
        .as_ref()
        .map(|(_, _, label, _)| label.as_str())
        .unwrap_or(fact.side_exit_label.as_str());
    let has_meta = ctx.block().icmp_ne(I64, &meta, "0");
    ctx.block()
        .cond_br(&has_meta, &object_spill_ptr_label, read_miss_label);

    ctx.current_block = object_spill_ptr_idx;
    let meta_ptr = ctx.block().inttoptr(I64, &meta);
    let spill_slot = ctx.block().gep(I64, &meta_ptr, &[(I64, "4")]);
    let spill = ctx.block().load(I64, &spill_slot);
    let has_spill = ctx.block().icmp_ne(I64, &spill, "0");
    let spill_deref_idx = ctx.new_block("stable_packed.load.object.spill_deref");
    let spill_deref_label = ctx.block_label(spill_deref_idx);
    ctx.block()
        .cond_br(&has_spill, &spill_deref_label, read_miss_label);

    ctx.current_block = spill_deref_idx;
    let spill_ptr = ctx.block().inttoptr(I64, &spill);
    let spill_len = ctx.block().load(I32, &spill_ptr);
    let spill_len64 = ctx.block().zext(I32, &spill_len, I64);
    let in_bounds = ctx.block().icmp_ult(I64, &slot, &spill_len64);
    let spill_load_idx = ctx.new_block("stable_packed.load.object.spill_load");
    let spill_load_label = ctx.block_label(spill_load_idx);
    ctx.block()
        .cond_br(&in_bounds, &spill_load_label, read_miss_label);

    ctx.current_block = spill_load_idx;
    let spill_word = ctx.block().add(I64, &slot, "1");
    let spill_element = ctx
        .block()
        .gep_inbounds(I64, &spill_ptr, &[(I64, &spill_word)]);
    let spill_value = ctx.block().load(DOUBLE, &spill_element);
    let spill_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    let direct = ctx.block().phi(
        DOUBLE,
        &[
            (&plain_value, &plain_end),
            (&inline_value, &inline_end),
            (&spill_value, &spill_end),
        ],
    );
    let resolved = finish_revalidated_read(ctx, direct, idx_i32.clone(), per_read_fallback);
    Some(finish_repeated_read_cache(
        ctx,
        resolved,
        idx_i32,
        repeated_read_cache,
        None,
        &u32_oob_label,
        fact.u32_component_bound.as_deref(),
    ))
}

type PerReadFallback = (usize, usize, String, String);
enum RepeatedReadCacheMiss {
    Populate(StablePackedReadCache),
    Lookup {
        cache: StablePackedReadCache,
        cached: String,
        hit_end: String,
        merge_idx: usize,
    },
}

/// Enter the miss arm of a same-counter value cache. A cached value is read
/// only when no semantic call has executed since it was produced; this is what
/// makes an unrooted boxed pointer safe under the moving collector as well as
/// preserving getters, proxies, and mutation between source occurrences.
fn begin_repeated_read_cache(
    ctx: &mut FnCtx<'_>,
    fact: &StablePackedLoopFact,
    idx_i32: &str,
) -> Option<RepeatedReadCacheMiss> {
    let mut cache = fact.repeated_read_cache.clone()?;
    let active = ctx
        .stable_packed_loop_facts
        .iter_mut()
        .rev()
        .find(|active| {
            active.array_local_id == fact.array_local_id
                && active.counter_local_id == fact.counter_local_id
        })?;
    if !active
        .repeated_read_cache
        .as_ref()
        .is_some_and(|active_cache| active_cache.has_producer)
    {
        active
            .repeated_read_cache
            .as_mut()
            .expect("active repeated-read cache")
            .has_producer = true;
        cache.has_producer = true;
        return Some(RepeatedReadCacheMiss::Populate(cache));
    }
    let dirty_slot = fact.revalidation_dirty_slot.as_ref()?;
    let valid = ctx.block().load(I1, &cache.valid_slot);
    let cached_counter = ctx.block().load(I32, &cache.counter_slot);
    let same_counter = ctx.block().icmp_eq(I32, &cached_counter, idx_i32);
    let dirty = ctx.block().load(I1, dirty_slot);
    let clean = ctx.block().icmp_eq(I1, &dirty, "0");
    let valid_and_same = ctx.block().and(I1, &valid, &same_counter);
    let hit = ctx.block().and(I1, &valid_and_same, &clean);
    let hit_idx = ctx.new_block("stable_packed.indexed_read.cache_hit");
    let miss_idx = ctx.new_block("stable_packed.indexed_read.cache_miss");
    let merge_idx = ctx.new_block("stable_packed.indexed_read.cache_merge");
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&hit, &hit_label, &miss_label);

    ctx.current_block = hit_idx;
    let cached = ctx.block().load(DOUBLE, &cache.value_slot);
    let hit_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = miss_idx;
    Some(RepeatedReadCacheMiss::Lookup {
        cache,
        cached,
        hit_end,
        merge_idx,
    })
}

/// Publish a miss value, then merge it with any previously emitted hit arm.
fn finish_repeated_read_cache(
    ctx: &mut FnCtx<'_>,
    resolved: String,
    idx_i32: String,
    cache_miss: Option<RepeatedReadCacheMiss>,
    native_u32: Option<&str>,
    side_exit_label: &str,
    component_bound: Option<&str>,
) -> String {
    let Some(cache_miss) = cache_miss else {
        return resolved;
    };
    let (cache, hit) = match cache_miss {
        RepeatedReadCacheMiss::Populate(cache) => (cache, None),
        RepeatedReadCacheMiss::Lookup {
            cache,
            cached,
            hit_end,
            merge_idx,
        } => (cache, Some((cached, hit_end, merge_idx))),
    };
    ctx.block().store(I32, &idx_i32, &cache.counter_slot);
    ctx.block().store(DOUBLE, &resolved, &cache.value_slot);
    if let Some(u32_slot) = cache.u32_slot.as_ref() {
        // Validate the entity id at its first source occurrence. A miss exits
        // before the conservative typed-array clone can perform an observable
        // effect, while hits reuse these exact native bits for every later
        // component access in the same source iteration.
        let canonical =
            emit_canonical_u32_guard(ctx, &resolved, native_u32, side_exit_label, component_bound);
        ctx.block().store(I32, &canonical, u32_slot);
    }
    ctx.block().store(I1, "1", &cache.valid_slot);
    let Some((cached, hit_end, merge_idx)) = hit else {
        return resolved;
    };
    let miss_end = ctx.block().label.clone();
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(DOUBLE, &[(&cached, &hit_end), (&resolved, &miss_end)])
}

/// Consume the exact-u32 prefix established by mode-2 runtime admission. That
/// persistent, mutation-invalidated proof makes `fptoui` defined here. A
/// shared component-length check below still keeps each direct typed-array
/// access within its owning allocation.
fn emit_canonical_u32_guard(
    ctx: &mut FnCtx<'_>,
    value: &str,
    native_u32: Option<&str>,
    out_of_bounds_label: &str,
    component_bound: Option<&str>,
) -> String {
    let canonical = native_u32
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| ctx.block().fptoui(DOUBLE, value, I32));
    if let Some(bound) = component_bound {
        let in_bounds = ctx.block().icmp_ult(I32, &canonical, bound);
        let continue_idx = ctx.new_block("stable_packed.component.in_bounds");
        let continue_label = ctx.block_label(continue_idx);
        ctx.block()
            .cond_br(&in_bounds, &continue_label, out_of_bounds_label);
        ctx.current_block = continue_idx;
    }
    canonical
}

fn u32_out_of_bounds_label(fact: &StablePackedLoopFact) -> &str {
    fact.u32_out_of_bounds_label
        .as_deref()
        .unwrap_or(&fact.side_exit_label)
}

/// Complete a nested-derived indexed read. The direct arm has already
/// consumed the live raw address with no intervening safepoint. On a guard or
/// defensive-layout miss, the ordinary arraylike helper performs exactly this
/// source read and rejoins without replaying any preceding statement effects.
fn finish_revalidated_read(
    ctx: &mut FnCtx<'_>,
    direct: String,
    idx_i32: String,
    fallback: Option<PerReadFallback>,
) -> String {
    let Some((fallback_idx, merge_idx, _, receiver)) = fallback else {
        return direct;
    };
    let direct_end = ctx.block().label.clone();
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().br(&merge_label);

    ctx.current_block = fallback_idx;
    let index = ctx.block().sitofp(I32, &idx_i32, DOUBLE);
    let generic = ctx.block().call(
        DOUBLE,
        "js_packed_arraylike_index_get",
        &[(DOUBLE, &receiver), (DOUBLE, &index), (PTR, "null")],
    );
    let fallback_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(DOUBLE, &[(&direct, &direct_end), (&generic, &fallback_end)])
}

use super::stable_packed_accumulator::collect_numeric_accumulators;

pub(crate) fn has_numeric_index_fact(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    let Expr::IndexGet { object, index } = expr else {
        return false;
    };
    let (Expr::LocalGet(array_id), Expr::LocalGet(counter_id)) = (object.as_ref(), index.as_ref())
    else {
        return false;
    };
    ctx.stable_packed_loop_facts.iter().rev().any(|fact| {
        fact.numeric_elements
            && fact.array_local_id == *array_id
            && fact.counter_local_id == *counter_id
    })
}

pub(crate) fn has_u32_index_fact(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    let Expr::IndexGet { object, index } = expr else {
        return false;
    };
    let (Expr::LocalGet(array_id), Expr::LocalGet(counter_id)) = (object.as_ref(), index.as_ref())
    else {
        return false;
    };
    ctx.stable_packed_loop_facts.iter().rev().any(|fact| {
        fact.u32_index_elements
            && fact.array_local_id == *array_id
            && fact.counter_local_id == *counter_id
    })
}

pub(crate) fn has_u32_component_bound(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    let Expr::IndexGet { object, index } = expr else {
        return false;
    };
    let (Expr::LocalGet(array_id), Expr::LocalGet(counter_id)) = (object.as_ref(), index.as_ref())
    else {
        return false;
    };
    ctx.stable_packed_loop_facts.iter().rev().any(|fact| {
        fact.u32_component_bound.is_some()
            && fact.array_local_id == *array_id
            && fact.counter_local_id == *counter_id
    })
}

/// Lower an admitted packed entity-id read and return its canonical native
/// u32 bits. Repeated source occurrences share both the guarded read and this
/// conversion through `StablePackedReadCache`.
pub(crate) fn try_lower_u32_index(ctx: &mut FnCtx<'_>, expr: &Expr) -> Option<String> {
    if !has_u32_index_fact(ctx, expr) {
        return None;
    }
    let Expr::IndexGet { object, index } = expr else {
        return None;
    };
    let value = try_lower_index_get(ctx, object, index)?;
    let (cache_slot, out_of_bounds_label, component_bound) = ctx
        .stable_packed_loop_facts
        .iter()
        .rev()
        .find(|fact| {
            matches!(
                (object.as_ref(), index.as_ref()),
                (Expr::LocalGet(array_id), Expr::LocalGet(counter_id))
                    if fact.array_local_id == *array_id
                        && fact.counter_local_id == *counter_id
            )
        })
        .map(|fact| {
            (
                fact.repeated_read_cache
                    .as_ref()
                    .and_then(|cache| cache.u32_slot.clone()),
                fact.u32_out_of_bounds_label
                    .clone()
                    .unwrap_or_else(|| fact.side_exit_label.clone()),
                fact.u32_component_bound.clone(),
            )
        })?;
    Some(if let Some(slot) = cache_slot {
        ctx.block().load(I32, &slot)
    } else {
        emit_canonical_u32_guard(
            ctx,
            &value,
            None,
            &out_of_bounds_label,
            component_bound.as_deref(),
        )
    })
}

/// Refresh a captured receiver at fast-iteration entry. The closure pointer is
/// reloaded through its GC root by ordinary `LocalGet` lowering, then the full
/// runtime admission rechecks identity, forwarding, layout, descriptors,
/// prototype state, packedness, and the admitted range. Only the returned live
/// address is published to direct indexed reads in this iteration.
pub(super) fn emit_iteration_guard(
    ctx: &mut FnCtx<'_>,
    loop_counter_id: Option<u32>,
) -> Result<bool> {
    let Some(fact) = ctx.stable_packed_loop_facts.last().cloned() else {
        return Ok(false);
    };
    if !fact.revalidate_each_iteration || loop_counter_id != Some(fact.counter_local_id) {
        return Ok(false);
    }

    let receiver = crate::expr::lower_expr(ctx, &Expr::LocalGet(fact.array_local_id))?;
    let live_raw = ctx.block().call(
        I64,
        "js_packed_arraylike_loop_revalidate_live",
        &[
            (DOUBLE, &receiver),
            (DOUBLE, &fact.bound),
            (
                I32,
                required_numeric_mode(fact.numeric_elements, fact.u32_index_elements),
            ),
            (PTR, &fact.descriptor),
        ],
    );
    let mut pass = ctx.block().icmp_ne(I64, &live_raw, "0");
    if fact.live_length_bound {
        let refreshed_bound = descriptor_word(ctx, &fact.descriptor, 6);
        let length_unchanged = ctx
            .block()
            .icmp_eq(I64, &refreshed_bound, &fact.admitted_bound);
        pass = ctx.block().and(I1, &pass, &length_unchanged);
    }

    let continue_idx = ctx.new_block("stable_packed.iteration.capture_valid");
    let continue_label = ctx.block_label(continue_idx);
    ctx.block()
        .cond_br(&pass, &continue_label, &fact.side_exit_label);
    ctx.current_block = continue_idx;

    let numeric_access = fact
        .numeric_elements
        .then(|| build_numeric_access(ctx, &fact.descriptor, &live_raw, fact.u32_index_elements));
    if let Some(active) = ctx.stable_packed_loop_facts.last_mut() {
        active.live_receiver_handle = Some(live_raw);
        active.numeric_access = numeric_access;
    }
    Ok(true)
}

pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Result<bool> {
    let Some(candidate) = match_candidate(ctx, init, condition, update, body) else {
        return Ok(false);
    };
    let typed_array_candidate = super::stable_packed_typed_array::find_candidate(
        ctx,
        body,
        candidate.array_id,
        candidate.counter_id,
        candidate.u32_index_elements,
    );
    // The stronger entity-id fact exists solely to feed the guarded column
    // view clone. Mode-2 admission establishes or consumes its persistent,
    // mutation-invalidated exact-u32 prefix proof.
    let u32_index_elements = typed_array_candidate.is_some();
    let inserted_counter = if ctx.i32_counter_slots.contains_key(&candidate.counter_id) {
        false
    } else {
        let Some(counter_slot) = ctx.locals.get(&candidate.counter_id).cloned() else {
            return Ok(false);
        };
        let slot = ctx.func.alloca_entry(I32);
        let value = ctx.block().load(DOUBLE, &counter_slot);
        let i32_value = ctx.block().fptosi(DOUBLE, &value, I32);
        ctx.block().store(I32, &i32_value, &slot);
        ctx.i32_counter_slots.insert(candidate.counter_id, slot);
        true
    };

    let receiver = crate::expr::lower_expr(ctx, &Expr::LocalGet(candidate.array_id))?;
    let bound_box = match candidate.bound {
        LoopBound::Snapshot(bound_id) => crate::expr::lower_expr(ctx, &Expr::LocalGet(bound_id))?,
        LoopBound::LiveLength => "-1.0".to_string(),
    };
    let descriptor = ctx
        .func
        .alloca_entry_array(I64, if u32_index_elements { 11 } else { 7 });
    let (admitted, admitted_live_raw, typed_array_admission) =
        if let Some(typed_array_candidate) = typed_array_candidate.as_ref() {
            let (admission, live_raw) = super::stable_packed_typed_array::emit_fused_admission(
                ctx,
                typed_array_candidate,
                &receiver,
                &bound_box,
                &descriptor,
            )?;
            (admission.guard.clone(), Some(live_raw), Some(admission))
        } else {
            let guard_args = [
                (DOUBLE, receiver.as_str()),
                (DOUBLE, bound_box.as_str()),
                (
                    I32,
                    required_numeric_mode(candidate.numeric_elements, false),
                ),
                (PTR, descriptor.as_str()),
            ];
            if candidate.capture_index.is_some() {
                let live_raw =
                    ctx.block()
                        .call(I64, "js_packed_arraylike_loop_guard_live", &guard_args);
                (
                    ctx.block().icmp_ne(I64, &live_raw, "0"),
                    Some(live_raw),
                    None,
                )
            } else {
                let guard = ctx
                    .block()
                    .call(I32, "js_packed_arraylike_loop_guard", &guard_args);
                (ctx.block().icmp_ne(I32, &guard, "0"), None, None)
            }
        };
    // The conservative column matcher proves the cloned body call-free, and
    // `fast_raw` below reloads the rooted derived receiver after every
    // admission helper has returned. Its packed layout therefore stays valid
    // for the complete clone. Keep the broader per-read revalidation tier for
    // nested generic bodies, but let this clone hoist all descriptor-derived
    // bases into its preheader.
    let revalidate_before_indexed_read =
        candidate.nested_requires_access_revalidation && typed_array_admission.is_none();
    // Deliberately left unterminated until the emitted fast clone has been
    // scanned. The cached receiver below is safe only when no runtime call can
    // allocate, collect, or revoke an admitted layout while that clone runs.
    let admission_idx = ctx.current_block;

    let fast_pre_idx = ctx.new_block("stable_packed.loop.fast.preheader");
    let slow_pre_idx = ctx.new_block("stable_packed.loop.slow.preheader");
    let merge_idx = ctx.new_block("stable_packed.loop.merge");
    let fast_pre_label = ctx.block_label(fast_pre_idx);
    let slow_pre_label = ctx.block_label(slow_pre_idx);
    let merge_label = ctx.block_label(merge_idx);

    let bound64 = {
        ctx.current_block = fast_pre_idx;
        descriptor_word(ctx, &descriptor, 6)
    };
    let bound_i32 = ctx.block().trunc(I64, &bound64, I32);
    // A capture reload is itself a runtime call, so its admission returns the
    // post-call live address. Ordinary addressable bindings retain the old
    // guard/reload sequence; their reload is a plain load and their clone must
    // still pass the call-free scan unless it has explicit access revalidation.
    let fast_raw = if let Some(live_raw) = admitted_live_raw {
        live_raw
    } else {
        let fast_receiver = crate::expr::lower_expr(ctx, &Expr::LocalGet(candidate.array_id))?;
        let fast_bits = ctx.block().bitcast_double_to_i64(&fast_receiver);
        ctx.block()
            .and(I64, &fast_bits, crate::nanbox::POINTER_MASK_I64)
    };
    let fast_scan_start = ctx.func.num_blocks();
    let installed_typed_array_views = typed_array_admission
        .as_ref()
        .map(|admission| super::stable_packed_typed_array::install_views(ctx, admission));
    let numeric_access = if candidate.numeric_elements {
        Some(build_numeric_access(
            ctx,
            &descriptor,
            &fast_raw,
            u32_index_elements,
        ))
    } else {
        None
    };
    // Reduce accumulators: one tag test each here in the fast preheader (the
    // induction base case), then the fact below carries the proof through the
    // fast clone so `s += arr[counter]` lowers to a native `fadd`. Admission
    // requires `numeric_elements` — without the element proof the accumulator
    // walk's `array[counter]` leaf has nothing to stand on.
    let numeric_accumulators = if candidate.numeric_elements {
        collect_numeric_accumulators(ctx, body, candidate.array_id, candidate.counter_id)
    } else {
        Vec::new()
    };
    if !numeric_accumulators.is_empty() {
        let mut all_numbers: Option<String> = None;
        for id in &numeric_accumulators {
            let slot = ctx
                .locals
                .get(id)
                .cloned()
                .expect("admitted accumulator has a plain slot");
            let value = ctx.block().load(DOUBLE, &slot);
            let is_number = super::loops::emit_js_value_is_number(ctx, &value);
            all_numbers = Some(match all_numbers {
                Some(prev) => ctx.block().and(I1, &prev, &is_number),
                None => is_number,
            });
        }
        let all_numbers = all_numbers.expect("at least one accumulator");
        let acc_ok_idx = ctx.new_block("stable_packed.acc.ok");
        let acc_ok_label = ctx.block_label(acc_ok_idx);
        // A non-Number accumulator (a string total, a BigInt) takes the slow
        // clone before the first fast iteration; nothing has run yet, so the
        // slow clone sees pristine state.
        ctx.block()
            .cond_br(&all_numbers, &acc_ok_label, &slow_pre_label);
        ctx.current_block = acc_ok_idx;
    }
    let revalidation_dirty_slot = candidate
        .nested_requires_access_revalidation
        .then(|| ctx.func.alloca_entry(I1));
    let revalidation_live_raw_slot = candidate
        .nested_requires_access_revalidation
        .then(|| ctx.func.alloca_entry(I64));
    let repeated_read_cache = candidate
        .cache_repeated_index_reads
        .then(|| StablePackedReadCache {
            valid_slot: ctx.func.alloca_entry(I1),
            counter_slot: ctx.func.alloca_entry(I32),
            value_slot: ctx.func.alloca_entry(DOUBLE),
            u32_slot: u32_index_elements.then(|| ctx.func.alloca_entry(I32)),
            has_producer: false,
        });
    if let Some(cache) = repeated_read_cache.as_ref() {
        ctx.block().store(I1, "0", &cache.valid_slot);
    }
    if let Some(slot) = revalidation_dirty_slot.as_ref() {
        // The admitting guard and the post-guard receiver reload establish a
        // clean proof. Calls emitted after this point dirty it at their actual
        // control-flow location via LlBlock's call choke points.
        ctx.block().store(I1, "0", slot);
        ctx.block().store(
            I64,
            &fast_raw,
            revalidation_live_raw_slot
                .as_ref()
                .expect("nested revalidation raw slot"),
        );
        ctx.func
            .reg_counter()
            .push_stable_packed_revalidation_slot(slot.clone());
    }
    ctx.stable_packed_loop_facts.push(StablePackedLoopFact {
        counter_local_id: candidate.counter_id,
        array_local_id: candidate.array_id,
        side_exit_label: slow_pre_label.clone(),
        descriptor,
        bound: bound_box,
        admitted_bound: bound64,
        live_length_bound: matches!(candidate.bound, LoopBound::LiveLength),
        revalidate_each_iteration: candidate.capture_index.is_some(),
        revalidate_before_indexed_read,
        revalidation_dirty_slot: revalidation_dirty_slot.clone(),
        revalidation_live_raw_slot,
        repeated_read_cache,
        live_receiver_handle: Some(fast_raw),
        numeric_elements: candidate.numeric_elements,
        u32_index_elements,
        u32_component_bound: installed_typed_array_views
            .as_ref()
            .map(|installed| installed.common_length.clone()),
        u32_out_of_bounds_label: None,
        numeric_access,
        numeric_accumulators,
        derived_locals: std::collections::HashSet::new(),
        u32_view_derived_locals: std::collections::HashMap::new(),
    });
    super::loops::lower_for_after_init_with_i32_bound(
        ctx,
        init,
        condition,
        update,
        body,
        "for.stable_packed_fast",
        Some((candidate.counter_id, bound_i32)),
    )?;
    ctx.stable_packed_loop_facts.pop();
    if let Some(installed) = installed_typed_array_views {
        super::stable_packed_typed_array::restore_views(ctx, installed);
    }
    if let Some(slot) = revalidation_dirty_slot.as_ref() {
        ctx.func
            .reg_counter()
            .pop_stable_packed_revalidation_slot(slot);
    }
    if !ctx.block().is_terminated() {
        // A call-free clone cannot grow or shrink its receiver, so exhausting
        // the admitted bound is also the exact live-length loop exit.
        ctx.block().br(&merge_label);
    }
    let fast_scan_end = ctx.func.num_blocks();
    let fast_clone_call_free = !ctx.func.blocks()[fast_pre_idx].contains_gc_unsafe_call()
        && (fast_scan_start..fast_scan_end)
            .all(|idx| !ctx.func.blocks()[idx].contains_gc_unsafe_call());
    ctx.current_block = admission_idx;
    let fast_clone_is_safe = fast_clone_call_free
        || candidate.capture_index.is_some()
        || candidate.nested_requires_access_revalidation;
    if fast_clone_is_safe {
        record_artifacts(ctx, &candidate, &receiver);
        ctx.block()
            .cond_br(&admitted, &fast_pre_label, &slow_pre_label);
    } else {
        ctx.block().br(&slow_pre_label);
    }

    ctx.current_block = slow_pre_idx;
    super::loops::lower_for_after_init(
        ctx,
        init,
        condition,
        update,
        body,
        "for.stable_packed_slow",
    )?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }
    ctx.current_block = merge_idx;
    if inserted_counter {
        ctx.i32_counter_slots.remove(&candidate.counter_id);
    }
    Ok(true)
}
