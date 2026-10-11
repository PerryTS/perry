//! The `new ClassName(...)` **instance allocation**, split out of `new.rs`
//! (#7615 slice 8).
//!
//! A pure move: this is `lower_new_impl_inner`'s field-count computation and
//! its three-arm object allocation, verbatim, wrapped in one function that
//! returns the raw instance handle. Nothing else in the `new` lowering reads
//! any of the locals it defines (`field_count`, `cid_str`, `parent_cid_str`,
//! `n_str`, `packed_keys`, `alloc_field_count`), which is what made the
//! boundary a boundary rather than a cut.
//!
//! The split exists because `new.rs` reached 1,988 lines against
//! `scripts/check_file_size.sh`'s 2,000-line cap, and the Layer 1 rooting
//! migration (#7615) has to ADD lines to it — `refresh_rooted_args` and the
//! `temp_root_scope_*` marker become a `RootedGroup`. Doing the move as its
//! own commit keeps that diff readable: this file has no rooting decision in
//! it at all, because everything it emits sits ABOVE the instance root (the
//! push is `new.rs`'s first act on the returned handle).

use perry_hir::Class;

use crate::expr::{load_inline_arena_state, FnCtx};
use crate::types::{I32, I64, I8, PTR};

/// An imported constructor can add slots the local keys image cannot describe.
/// Keep its cold births on the runtime path, which consumes learned widths.
/// This is a compile-time fact check; existing hot-site admission is unchanged.
pub(super) fn cold_birth_constructor_chain_visible<'c>(
    class_name: &str,
    class: &'c Class,
    lookup: &dyn Fn(&str) -> Option<&'c Class>,
    imported: &dyn Fn(&str) -> bool,
) -> bool {
    let mut current = class;
    let mut name = class_name;
    for _ in 0..32 {
        if imported(name) || current.extends_expr.is_some() || current.native_extends.is_some() {
            return false;
        }
        match current.extends_name.as_deref() {
            None => return true,
            Some(parent) => {
                let Some(next) = lookup(parent) else {
                    return false;
                };
                current = next;
                name = parent;
            }
        }
    }
    false
}

/// #7469: is the `new` site being lowered inside a **loop body**?
///
/// This is the hot-site admission for larger inline births. Small exact
/// births bypass this heuristic. Inlining removes the
/// `js_object_alloc_class_inline_keys` FFI call and, with it, the thread-local
/// resolutions that call performs — measured 1.81× on `churn_alloc`, 1.78× on
/// `push_cls`. But it costs ~268 bytes of machine code **per site** (measured
/// over 10 / 200 / 800-site programs: +0, +49,536, +214,656 bytes), so it
/// cannot be the unconditional default against this repo's binary-size
/// campaign — a 5,000-site application would pay ~1.3 MB.
///
/// Loop membership is the cheapest sound proxy for "this site runs many
/// times", and it bounds the size cost to loop bodies. A `new` executed once
/// keeps the outlined call and contributes nothing to binary growth.
///
/// `loop_targets` is reused rather than a new counter added, because it is
/// already maintained by all three loop lowerings. It also carries `switch`
/// frames, which push an EMPTY continue label (`switch_stmt.rs`) while every
/// loop pushes a real one — the same discriminator `Stmt::Continue`'s
/// scan-outward-past-switch-frames logic uses. A `new` inside a bare `switch`
/// is therefore correctly treated as not-in-a-loop.
fn new_site_is_in_loop(ctx: &FnCtx<'_>) -> bool {
    if ctx
        .loop_targets
        .iter()
        .any(|(continue_label, _, _)| !continue_label.is_empty())
    {
        return true;
    }
    // #7834: a `new` in a function the hot-loop-callee pre-pass admitted is a
    // `new` in a loop, one frame out.
    //
    // The gate below this comment is about SPEED-vs-SIZE, and
    // `collect_hot_loop_callees` answers exactly the question the loop test
    // does — is this site hot enough to be worth ~268 bytes — with the
    // anti-bloat backstop already attached: it admits only a function that
    // (a) has a direct call site inside a loop and (b) has at most
    // `inline_hot_small_max_call_sites` (4) direct call sites in the whole
    // module. So the added code is bounded by 4 × (news in the function),
    // which is the same order the loop arm already accepts.
    //
    // Deliberately NOT `func.inline_hint`: that is this set intersected with a
    // 9..=20-statement window and with "not already `alwaysinline`", and the
    // functions this needs most fall out of BOTH. `makeCycle` is 5 statements,
    // so it is `alwaysinline` and never hinted — while being the single
    // hottest function in the program.
    //
    // `cycles.ts` is the shape that needs it: `makeCycle` is called 10M times
    // from `main`'s loop and allocates two `Cell`s, but its own body has no
    // loop, so both allocations took the outlined
    // `js_object_alloc_class_inline_keys` — 22% of the program's samples, plus
    // a further 5% in `arena_alloc`'s inline-state sync, for work the inline
    // bump does in eight stores.
    //
    // Reading `func.hot_loop_callee` here is well-ordered: `codegen/function.rs`
    // sets it from `cross_module.hot_loop_callees` before the entry block is
    // created and before any expression is lowered.
    if ctx.func.hot_loop_callee {
        return true;
    }
    // #7871: the same question, asked with the right cost model.
    //
    // `hot_loop_callee` above carries `inline_hot_small_max_call_sites` (4),
    // which is `inlinehint`'s anti-bloat backstop — it bounds a cost that
    // scales with CALL SITES because LLVM duplicates the callee body at each
    // one. The inline bump allocator's cost is ~268 bytes per `new` SITE in
    // this function, paid once regardless of how many callers there are. So
    // the cap prices a cost that does not exist here, and it excludes exactly
    // the functions that earn the inline form: a recursive-descent evaluator's
    // hot function has one call site per recursion arm.
    //
    // `gc-handoff/apps/interp.ts`'s `evalNode` had 11 (10 of them its own
    // recursion) and allocated a `Value` per invocation through the outlined
    // call, ~20M times. Whole-corpus A/B with `PERRY_INLINE_NEW=1` (the
    // force-everywhere knob, i.e. a strict superset of this rule): `interp`
    // −16.2%, `iso_miss` −10.4%, `pipeline` −8.4%, zero regressions outside a
    // ±1.6% floor — and 15 of 19 binaries came out byte-identical, so the
    // widening reaches four programs, not the corpus.
    ctx.func.alloc_hot
}

/// Whether the raw inline allocator can publish the class's pre-minted
/// descriptor without asking the runtime to repair its live-slot facts.
fn inline_shape_descriptor_facts_exact(
    canonical_key_count: Option<u32>,
    allocation_field_count: u32,
) -> bool {
    canonical_key_count.is_some_and(|key_count| key_count == allocation_field_count)
}

/// Emit the instance allocation for `new <class_name>(...)` and return the raw
/// object handle (an `i64` user pointer, NOT NaN-boxed).
///
/// Classes with named fields or a dynamic parent reserve slots on a keyless
/// birth. Other classes have empty canonical keys and retain their inline bump
/// or outlined allocator; synthetic classes use the packed-keys fallback.
///
/// **No rooting decision is made here and none is possible.** The returned
/// handle is live in an SSA register only until the caller's very next
/// emission, which is the `RootedGroup::adopt_emitted` push that roots it for
/// the constructor body; nothing between the allocator call and that push can
/// collect.
/// What [`emit_instance_alloc`] produced: the instance's user pointer, plus
/// whether the allocation already stamped this class's canonical typed-shape
/// layout into the object's `GcHeader` constant (#7834).
pub(super) struct InstanceAlloc {
    pub(super) handle: String,
    /// `true` when the allocator publishes this class's canonical keys and
    /// ShapeId at birth. Both the inline bump and the stamped outlined
    /// allocator provide the structural proof needed by constructor-free
    /// field initialization.
    pub(super) constructor_stores_ready: bool,
}

/// The number of distinct static keys the constructor chain of `class` stores
/// into `this` that no class of the chain declares as a field. Walks each
/// constructor body's statements (branches, loops and `try` included; nested
/// functions excluded, since their `this` is not the instance). Capped, so a
/// generated constructor cannot make an instance arbitrarily wide.
pub(crate) fn constructor_added_key_count(ctx: &FnCtx<'_>, class: &perry_hir::Class) -> u32 {
    constructor_added_key_count_in(class, &|name| ctx.classes.get(name).copied())
}

/// One capacity derivation for allocation sites, literal descriptors and the
/// module-init birth image. Anonymous additions have already been normalized
/// over classes sharing a keys global, so aliases get the same live bound.
pub(crate) fn birth_slack_in<'c>(
    class: &'c Class,
    lookup: &dyn Fn(&str) -> Option<&'c Class>,
    anon_adds: &std::collections::HashMap<String, std::collections::BTreeSet<String>>,
) -> u32 {
    constructor_added_key_count_in(class, lookup)
        + private_field_slot_count_in(class, lookup)
        + anon_adds
            .get(&class.name)
            .map_or(0, |keys| keys.len().min(8) as u32)
}

/// The private fields construction claims on an instance of `class`: one
/// entry each, appended after the birth keys (#11791). Like the constructor
/// key-adds they get in-object slack, so every private field is an inline
/// slot. Counted over the local chain; an unresolved ancestor contributes
/// nothing (its fields land in overflow, which stays correct).
pub(crate) fn private_field_slot_count_in<'c>(
    class: &'c perry_hir::Class,
    lookup: &dyn Fn(&str) -> Option<&'c perry_hir::Class>,
) -> u32 {
    let mut count = 0u32;
    let mut current = Some(class);
    let mut depth = 0;
    while let Some(c) = current {
        if c.is_imported_stub() || depth > 32 {
            break;
        }
        count += c.fields.iter().filter(|f| f.is_private).count() as u32;
        depth += 1;
        current = c.extends_name.as_deref().and_then(lookup);
    }
    count.min(64)
}

/// [`constructor_added_key_count`] over any class table: module init
/// (`codegen/mod.rs`) derives the same count from its own table to mint the
/// wide birth shape the inline allocator stamps.
pub(crate) fn constructor_added_key_count_in<'c>(
    class: &'c perry_hir::Class,
    lookup: &dyn Fn(&str) -> Option<&'c perry_hir::Class>,
) -> u32 {
    const SLACK_CAP: usize = 64;
    let mut chain: Vec<&perry_hir::Class> = vec![class];
    let mut parent = class.extends_name.as_deref();
    while let Some(name) = parent {
        match lookup(name) {
            Some(p) if chain.len() < 32 => {
                chain.push(p);
                parent = p.extends_name.as_deref();
            }
            _ => break,
        }
    }
    let declared: std::collections::HashSet<&str> = chain
        .iter()
        .flat_map(|c| c.fields.iter().map(|f| f.name.as_str()))
        .collect();
    let mut added: std::collections::HashSet<String> = std::collections::HashSet::new();
    fn visit_expr(
        e: &perry_hir::Expr,
        declared: &std::collections::HashSet<&str>,
        added: &mut std::collections::HashSet<String>,
    ) {
        let key = match e {
            perry_hir::Expr::PropertySet {
                object, property, ..
            } if matches!(object.as_ref(), perry_hir::Expr::This) => Some(property.as_str()),
            perry_hir::Expr::PutValueSet { target, key, .. }
                if matches!(target.as_ref(), perry_hir::Expr::This) =>
            {
                match key.as_ref() {
                    perry_hir::Expr::String(k) => Some(k.as_str()),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(k) = key {
            let first = k.as_bytes().first().copied().unwrap_or(b'0');
            if !declared.contains(k) && first != b'#' && !first.is_ascii_digit() {
                added.insert(k.to_string());
            }
        }
        if matches!(e, perry_hir::Expr::Closure { .. }) {
            return;
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| visit_expr(c, declared, added));
    }
    fn visit_stmts(
        stmts: &[perry_hir::Stmt],
        declared: &std::collections::HashSet<&str>,
        added: &mut std::collections::HashSet<String>,
    ) {
        use perry_hir::Stmt;
        for s in stmts {
            match s {
                Stmt::Expr(e) => visit_expr(e, declared, added),
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    visit_stmts(then_branch, declared, added);
                    if let Some(eb) = else_branch {
                        visit_stmts(eb, declared, added);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                    visit_stmts(body, declared, added)
                }
                Stmt::For { body, .. } => visit_stmts(body, declared, added),
                Stmt::Labeled { body, .. } => {
                    visit_stmts(std::slice::from_ref(body.as_ref()), declared, added)
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    visit_stmts(body, declared, added);
                    if let Some(c) = catch {
                        visit_stmts(&c.body, declared, added);
                    }
                    if let Some(f) = finally {
                        visit_stmts(f, declared, added);
                    }
                }
                _ => {}
            }
        }
    }
    for c in &chain {
        if let Some(ctor) = &c.constructor {
            visit_stmts(&ctor.body, &declared, &mut added);
        }
        if added.len() >= SLACK_CAP {
            break;
        }
    }
    added.len().min(SLACK_CAP) as u32
}

/// A literal is materialized atomically from already evaluated values. A
/// root's bare fields can likewise be defined by its birth fills when the
/// complete constructor is an unobservable assignment prologue. No inferred
/// constructor property is included in this set: HIR retains actual fields only.
pub(crate) fn keys_defined_at_birth(class: &Class) -> bool {
    !class
        .fields
        .iter()
        .any(|f| f.origin == perry_hir::ClassFieldOrigin::ConstructorStore)
        && (class.is_literal_shape()
            || (class
                .constructor
                .as_ref()
                .is_some_and(|ctor| ctor.body.len() == class.fields.len())
                && !class.fields.is_empty()
                && super::field_init::ctor_prologue_param_assigned_fields(class).len()
                    == class.fields.len()))
}

pub(super) fn emit_instance_alloc(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    class: &Class,
) -> InstanceAlloc {
    let mut constructor_stores_ready = false;
    let handle = emit_instance_alloc_inner(ctx, class_name, class, &mut constructor_stores_ready);
    InstanceAlloc {
        handle,
        constructor_stores_ready,
    }
}

fn emit_instance_alloc_inner(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    class: &Class,
    constructor_stores_ready: &mut bool,
) -> String {
    // Compute total field count including inherited parent fields.
    // The runtime allocates at least 8 inline slots regardless, so this
    // mostly matters for shapes >8 fields.
    let public_keyable_count = |fields: &[perry_hir::ClassField]| -> u32 {
        fields
            .iter()
            .filter(|field| field.key_expr.is_none() && !field.is_private)
            .count() as u32
    };
    let mut field_count = public_keyable_count(&class.fields);
    let mut has_public_fields = field_count != 0;
    // Imported classes now carry their real field_names from the source
    // module. If the field count is still 0 (no fields info available),
    // use a generous default as a safety net.
    if field_count == 0 && class.constructor.is_none() {
        field_count = 32;
    }
    let mut parent = class.extends_name.as_deref();
    while let Some(parent_name) = parent {
        if let Some(p) = ctx.classes.get(parent_name).copied() {
            has_public_fields |= public_keyable_count(&p.fields) != 0;
            field_count += public_keyable_count(&p.fields);
            parent = p.extends_name.as_deref();
        } else {
            break;
        }
    }
    // Issue #26 / #321: prefer the authoritative per-class field count computed
    // by the source-prefix-disambiguated keys-global builder. The walk above
    // resolves parents via `ctx.classes` — a name-keyed map that holds only
    // ONE same-named stub — so when a cross-module parent name collides
    // (effect's `Type` in SchemaAST.ts vs ParseResult.ts) it counts the wrong
    // parent's fields. Using the keys-global's count keeps the allocated slot
    // count and the header `field_count` in lockstep with the keys array,
    // which `Object.keys()` walks. Falls back to the computed walk when this
    // class has no keys global (anonymous / no-keys path).
    if let Some(&authoritative) = ctx.class_field_counts.get(class_name) {
        has_public_fields |= authoritative != 0;
        field_count = authoritative;
    }
    // #6812 (w16): a per-site empty-literal anon-shape class may carry a
    // compile-time proven builder width. Allocate that many inline slots so
    // the FIRST instance of the site is as wide as the runtime-learned
    // resizes make every later one — a lone under-sized first instance
    // permanently vetoes whole-loop clone eligibility for arrays built at
    // the site. Capacity only: the keys array stays authoritative for
    // enumeration, and the runtime treats header field_count as alloc_limit.
    if class.alloc_width_hint > field_count {
        field_count = class.alloc_width_hint;
    }
    // In-object slack for KEY-ADDS: a constructor chain that stores keys the
    // class does not declare (`this.pos = pos` in a class with no field
    // declarations, tsc's and Zod's normal form) would otherwise spill every
    // key past INLINE_SLOT_FLOOR to overflow storage. The in-loop inline
    // allocator below bakes `field_count` at compile time and never consults
    // the runtime's learned width, so without this every instance of such a
    // class is born two slots wide forever (measured: 8 of 10 constructor
    // adds overflowed). Capacity only, exactly like `alloc_width_hint`: the
    // keys stay authoritative for enumeration, and a width above the keys
    // count routes the allocation to the outlined entry, which installs an
    // exact descriptor and also honours the learned width.
    let slack = birth_slack_in(
        class,
        &|name| ctx.classes.get(name).copied(),
        ctx.anon_key_adds,
    );
    if slack > 0 {
        field_count = field_count.max(
            ctx.class_field_counts
                .get(class_name)
                .copied()
                .unwrap_or(field_count)
                .saturating_add(slack),
        );
    }

    // Allocate the object with the per-class id and (if applicable)
    // parent class id, so the runtime registers the inheritance
    // chain for instanceof / virtual dispatch lookups.
    //
    // Class keys describe the final layout used by guarded field access.
    // The allocation image instead describes actual birth membership: empty
    // for a class whose definitions still have to execute, or the final keys
    // when an unobservable definition phase can be fused into birth fills.
    //
    // The packed-keys constant is interned via the StringPool. Two
    // classes with the same field-name set + order share one constant.
    let cid = ctx.class_ids.get(class_name).copied().unwrap_or(0);
    let parent_cid = class
        .extends_name
        .as_deref()
        .and_then(|p| ctx.class_ids.get(p).copied())
        .unwrap_or(0);
    let cid_str = cid.to_string();
    let parent_cid_str = parent_cid.to_string();
    let n_str = field_count.to_string();

    // Field names describe a possible layout, not properties already owned by
    // the newborn. DefineField and constructor stores establish membership in
    // execution order (including ancestor stores before derived fields).
    // Keep the allocation width, but publish an empty-key birth. Dynamic
    // parents need the same rule; unknown ancestor stores can safely spill.
    let deferred_keys = has_public_fields && !keys_defined_at_birth(class);
    if class.extends_expr.is_some()
        || (deferred_keys && !ctx.class_header_image_globals.contains_key(class_name))
    {
        return ctx.block().call(
            I64,
            "js_object_alloc_with_parent",
            &[(I32, &cid_str), (I32, &parent_cid_str), (I32, &n_str)],
        );
    }

    // Fast path: if the class has a per-class keys global (built once
    // at module init via `js_build_class_keys_array`), emit INLINE
    // bump-allocator IR — no function call into the runtime at all on
    // the hot path. The runtime exposes a `InlineArenaState` struct
    // (data ptr at offset 0, current bump offset at offset 8, current
    // block size at offset 16) via `js_inline_arena_state()`. Ordinary
    // allocation kernels cache that pointer at entry; self-recursive
    // allocators receive it from their public wrapper and forward it through
    // recursive calls. We then emit a 5-instruction bump check +
    // GcHeader/ObjectHeader
    // store sequence at every `new ClassName()` site. The slow path
    // (block overflow) calls `js_inline_arena_slow_alloc` which syncs
    // the inline state back to the underlying arena, allocates a new
    // block, and updates the inline state.
    //
    // Cycles per inlined alloc on the M-series fast path:
    //    load offset       (1)
    //    add+and align     (2)
    //    add new_offset    (1)
    //    load size + cmp   (2)
    //    cond br           (predicted, 0)
    //    store offset      (1)
    //    load data + gep   (2)
    //    write GcHeader    (1)  — packed i64 store
    //    write ObjectHeader (1)  — class id + ShapeId
    //    write null meta    (1)
    //  total: ~11 cycles vs ~140 cycles for the function-call path.
    //
    // Layout assumption: GcHeader is 8 bytes
    //    {obj_type:u8, gc_flags:u8, _reserved:u16, size:u32}
    // and ObjectHeader is 16 bytes on LP64 and ILP32 (#8047)
    //    {class_id:u32, parent_class_id:u32, meta:*ptr [, ILP32 pad:u32]}
    // followed by `max(field_count, INLINE_SLOT_FLOOR)` 8-byte field
    // slots. The user pointer the rest of the codegen sees is `raw + 8`
    // (i.e. the ObjectHeader address) — same as what
    // `js_object_alloc_class_inline_keys` returns.
    //
    // #8113 note on the SHAPE WORD: `parent_class_id` carries the
    // module-init ShapeId, and that descriptor is now the ONLY record of
    // the object's live inline-slot bound. The `descriptor_facts_exact`
    // gate below is therefore load-bearing, not an optimization: an
    // inline allocation whose slot bound differs from the id's descriptor
    // would publish an object the runtime bounds-checks against the WRONG
    // number. Mismatches take the outlined
    // `js_object_alloc_class_inline_keys_stamped` entry point, which
    // installs an exact local descriptor.
    //
    // Layout constants are duplicated here from the runtime; if
    // `GcHeader` or `ObjectHeader` ever change in
    // `crates/perry-runtime/src/{gc,object}.rs`, update both sides.
    if let Some(keys_global_name) = ctx.class_keys_globals.get(class_name).cloned() {
        // Both arms below stamp the same birth ShapeId. The
        // outlined arm may allocate an old-generation object when a learned
        // width crosses the large-object threshold; constructor-free pointer
        // stores retain the ordinary generation-tested write barrier.
        *constructor_stores_ready = !deferred_keys;
        // Exact small births use emitted allocation even in callbacks and
        // cross-module factories, where lexical loop membership says nothing
        // about execution frequency. Large sites retain the established
        // hot-site admission and its experimental force flag. Descriptor and
        // header-size agreement below remain mandatory on every inline site.
        let force_inline_new = std::env::var_os("PERRY_INLINE_NEW").is_some();
        // #8067: the raw inline allocator cannot ask the runtime to validate
        // descriptor facts after writing the ShapeId. Admit it only when the
        // allocation's live-slot bound exactly equals the module-init keys
        // count used to mint that id. Width-hinted/mismatched allocations use
        // the outlined entry point, which installs an exact local descriptor.
        // A class whose constructor adds keys is born WIDE: module init mints
        // its birth ShapeId with the widened live bound and composes the
        // header image for the widened size (`codegen/mod.rs`). The inline
        // allocator may stamp that image only when it is byte-for-byte the
        // one this site would build; and no site may stamp a module image
        // whose object SIZE differs from its own, since the image's ShapeId
        // then names a live bound this allocation may not have.
        let site_total =
            crate::target_layout::inline_alloc_total_size_bytes(ctx.target_triple, field_count);
        let module_image = ctx
            .class_header_image_globals
            .get(class_name)
            .map(|(_, packed, image_cid)| (*packed, *image_cid));
        let image_size_agrees = module_image.is_none_or(|(packed, _)| packed >> 32 == site_total);
        let wide_birth_image = slack > 0
            && module_image
                == Some((
                    crate::target_layout::inline_alloc_gc_packed(ctx.target_triple, field_count),
                    cid,
                ));
        let descriptor_facts_exact = (inline_shape_descriptor_facts_exact(
            ctx.class_field_counts.get(class_name).copied(),
            field_count,
        ) && image_size_agrees)
            || wide_birth_image;
        // ILP32 (wasm32 WASI, #11378): the inline bump below reads
        // `InlineArenaState` at LP64 offsets; take the outlined call.
        // Small, exactly described births use the same emitted Eden bump
        // regardless of the caller's lexical loop visibility. The runtime's
        // small-birth cutoff is 16 KiB; retain the old admission for larger
        // sites rather than changing their generation/allocation policy.
        let small_birth = site_total < 16 * 1024
            && cold_birth_constructor_chain_visible(
                class_name,
                class,
                &|name| ctx.classes.get(name).copied(),
                &|name| ctx.imported_class_ctors.contains_key(name),
            );
        if !descriptor_facts_exact
            || (!small_birth && !force_inline_new && !new_site_is_in_loop(ctx))
            || crate::codegen::helpers::ilp32_target()
        {
            let (keys_ptr, shape_id) = if deferred_keys {
                let image_global = ctx.class_header_image_globals[class_name].0.clone();
                let blk = ctx.block();
                let image = blk.load("<2 x i64>", &format!("@{image_global}"));
                let word = blk.next_reg();
                blk.emit_raw(format!("{word} = extractelement <2 x i64> {image}, i32 1"));
                let shifted = blk.lshr(I64, &word, "32");
                ("0".to_string(), blk.trunc(I64, &shifted, I32))
            } else {
                let keys_slot = if let Some(s) = ctx.class_keys_slots.get(class_name).cloned() {
                    s
                } else {
                    let s = crate::expr::entry_init_load_rooted_global(ctx, &keys_global_name, I64);
                    ctx.class_keys_slots
                        .insert(class_name.to_string(), s.clone());
                    s
                };
                let keys_bits = ctx.block().load(I64, &keys_slot);
                let keys_ptr =
                    ctx.block()
                        .and(I64, &keys_bits, &crate::nanbox::POINTER_MASK.to_string());
                let shape_id =
                    crate::typed_shape::load_class_shape_id(ctx, class_name, &keys_global_name);
                (keys_ptr, shape_id)
            };
            ctx.pending_declares.push((
                "js_object_alloc_class_inline_keys_stamped".to_string(),
                I64,
                vec![I32, I32, I32, I64, I32, I64],
            ));
            // The birth rep module init minted that id with (T1): a birth
            // the runtime cannot stamp with it verbatim still carries it.
            let rep = if deferred_keys {
                0
            } else {
                ctx.class_birth_reps
                    .get(&keys_global_name)
                    .copied()
                    .unwrap_or(0)
            }
            .to_string();
            ctx.block().call(
                I64,
                "js_object_alloc_class_inline_keys_stamped",
                &[
                    (I32, &cid_str),
                    (I32, &parent_cid_str),
                    (I32, &field_count.to_string()),
                    (I64, &keys_ptr),
                    (I32, &shape_id),
                    (I64, &rep),
                ],
            )
        } else {
            let total_size =
                crate::target_layout::inline_alloc_total_size_bytes(ctx.target_triple, field_count);
            let total_size_str = total_size.to_string();

            // Inline bump-allocator IR.
            let state_ptr = load_inline_arena_state(ctx);
            let blk = ctx.block();

            // offset = state.offset (at byte offset 8 in InlineArenaState).
            // The offset is invariant 8-aligned: arena blocks start at offset 0
            // (8-aligned), every allocation is a multiple of 8 (`total_size`
            // includes the 8-byte GcHeader and `MIN_FIELD_SLOTS=2` slots ×
            // 8 bytes), and `js_inline_arena_slow_alloc` only ever swings the
            // state to `block.offset` which is also always 8-aligned. So we
            // skip the `(offset + 7) & -8` align-up step entirely — saves
            // 2 instructions per iter on the hot path.
            let offset_field_ptr = blk.gep(I8, &state_ptr, &[(I64, "8")]);
            let offset_val = blk.load(I64, &offset_field_ptr);
            let aligned_off = offset_val.clone();

            // new_offset = aligned + total_size
            let new_offset = blk.add(I64, &aligned_off, &total_size_str);

            // size = state.size (at byte offset 16)
            let size_field_ptr = blk.gep(I8, &state_ptr, &[(I64, "16")]);
            let size_val = blk.load(I64, &size_field_ptr);

            // fits = new_offset <= size
            let fits = blk.icmp_ule(I64, &new_offset, &size_val);

            // Write the 16-byte header prefix — GcHeader (8 bytes) followed by
            // the first ObjectHeader word — with ONE `<2 x i64>` store.
            //
            // GcHeader packing (little-endian):
            //   bits  0..7   = obj_type (u8)
            //   bits  8..15  = gc_flags (u8)
            //   bits 16..31  = _reserved (u16)
            //   bits 32..63  = size (u32)
            //
            // The ObjectHeader word at raw + 8: #8113 collapsed the two packed
            // words into one: `class_id` (u32, low) | ShapeId (u32, high).
            // The module-init runtime call either publishes a usable ShapeId
            // or fail-stops on exhaustion; there is no pointer-token fallback.
            // The deleted `object_type` was a constant and the deleted
            // `field_count` is now the ShapeId descriptor's
            // `live_inline_slot_count`, which the `descriptor_facts_exact`
            // gate above proved equals this site's `field_count`.
            //
            // #8122: the pair is composed ONCE per function
            // (`LlFunction::entry_init_object_header_image`) rather than
            // stored as two scalars here. With `object_type` gone the second
            // word is no longer a constant, so two scalar stores made LLVM
            // rematerialise the 40-bit `gc_packed` immediate at every
            // allocation (`mov` + two `movk`) and shift/or the ShapeId per
            // site — measured +4.5 instructions per `new`. The vector image is
            // one live register (or one reload) and one `str q` per
            // allocation, the shape the pre-#8113 constant pair compiled to.
            let gc_packed: u64 =
                crate::target_layout::inline_alloc_gc_packed(ctx.target_triple, field_count);
            // Prefer the module-level image global — composed once at module
            // init from the SAME `inline_alloc_gc_packed` derivation — and use
            // it only when the table's packed word equals this site's own. A
            // header word is not something to take on trust; a mismatch means
            // the per-function compose (below) is used instead of a wrong
            // header. `image_slot` is an entry-hoisted copy (like the keys
            // global), so a site inside a loop or a recursive allocator pays
            // one vector load per function entry and one `str q` per `new`.
            let image_key = (class_name.to_string(), gc_packed);
            let image_source = if let Some(source) = ctx.class_header_images.get(&image_key) {
                source.clone()
            } else {
                let module_image = ctx
                    .class_header_image_globals
                    .get(class_name)
                    .filter(|(_, module_gc_packed, module_cid)| {
                        *module_gc_packed == gc_packed && *module_cid == cid
                    })
                    .map(|(global, _, _)| global.clone());
                let source = if let Some(image_global) = module_image {
                    crate::expr::HeaderImageSource::EntrySlot(
                        ctx.func.entry_init_load_global(&image_global, "<2 x i64>"),
                    )
                } else {
                    // Fallback: compose the pair once per function from the
                    // ShapeId global's entry slot.
                    let shape_slot = crate::typed_shape::ensure_class_shape_slot(
                        ctx,
                        class_name,
                        &keys_global_name,
                    );
                    crate::expr::HeaderImageSource::EntryValue(
                        ctx.func
                            .entry_init_object_header_image(&shape_slot, gc_packed, cid),
                    )
                };
                ctx.class_header_images.insert(image_key, source.clone());
                source
            };
            let birth_rep = if deferred_keys {
                0
            } else {
                ctx.class_birth_reps
                    .get(&keys_global_name)
                    .copied()
                    .unwrap_or(0)
            };
            let header_image = match image_source {
                crate::expr::HeaderImageSource::EntrySlot(slot) => {
                    ctx.block().load("<2 x i64>", &slot)
                }
                crate::expr::HeaderImageSource::EntryValue(value) => value,
            };
            let fast_idx = ctx.new_block("alloc.fast");
            let slow_idx = ctx.new_block("alloc.slow");
            let merge_idx = ctx.new_block("alloc.merge");
            let fast_label = ctx.block_label(fast_idx);
            let slow_label = ctx.block_label(slow_idx);
            let merge_label = ctx.block_label(merge_idx);
            ctx.block().cond_br(&fits, &fast_label, &slow_label);
            ctx.current_block = fast_idx;
            let blk = ctx.block();
            // GC_STORE_AUDIT(INIT): arena allocator metadata, not a JS edge.
            blk.store(I64, &new_offset, &offset_field_ptr);
            let data_ptr = blk.load(PTR, &state_ptr);
            let raw_fast = blk.gep(I8, &data_ptr, &[(I64, &aligned_off)]);
            // GC_STORE_AUDIT(INIT): unpublished base header. The shared
            // routine composes live color before publication or a GC call.
            ctx.block().emit_raw(format!(
                "store <2 x i64> {header_image}, ptr {raw_fast}, align 8"
            ));
            let fast_handle = crate::expr::inline_birth::class(
                ctx,
                &raw_fast,
                &state_ptr,
                &header_image,
                birth_rep,
                true,
            );
            let fast_pred = ctx.block().label.clone();
            ctx.block().br(&merge_label);
            ctx.current_block = slow_idx;
            let slow_handle = crate::expr::inline_birth::class(
                ctx,
                "null",
                &state_ptr,
                &header_image,
                birth_rep,
                false,
            );
            let slow_pred = ctx.block().label.clone();
            ctx.block().br(&merge_label);
            ctx.current_block = merge_idx;
            ctx.block().phi(
                I64,
                &[(&fast_handle, &fast_pred), (&slow_handle, &slow_pred)],
            )
        }
    } else {
        // Fallback: build the packed-keys string at this site and
        // call the slower SHAPE_CACHE-aware allocator. Used when the
        // class isn't in `class_keys_globals` (e.g. anonymous /
        // synthetic classes that compile_module doesn't pre-emit a
        // global for).
        let mut packed_keys = String::new();
        let mut parent_chain: Vec<&perry_hir::Class> = Vec::new();
        let mut p = class.extends_name.as_deref();
        while let Some(parent_name) = p {
            if let Some(pc) = ctx.classes.get(parent_name).copied() {
                parent_chain.push(pc);
                p = pc.extends_name.as_deref();
            } else {
                break;
            }
        }
        // Skip computed and private fields: both are initialized through
        // dedicated runtime paths and neither belongs in the public inline
        // shape exposed by Object.keys/getOwnPropertyNames.
        for pc in parent_chain.iter().rev() {
            for f in &pc.fields {
                if f.key_expr.is_some() || f.is_private {
                    continue;
                }
                packed_keys.push_str(&f.name);
                packed_keys.push('\0');
            }
        }
        for f in &class.fields {
            if f.key_expr.is_some() || f.is_private {
                continue;
            }
            packed_keys.push_str(&f.name);
            packed_keys.push('\0');
        }
        let keys_idx = ctx.strings.intern(&packed_keys);
        let keys_entry = ctx.strings.entry(keys_idx);
        let keys_global = format!("@{}", keys_entry.bytes_global);
        let keys_len_str = keys_entry.byte_len.to_string();

        ctx.block().call(
            I64,
            "js_object_alloc_class_with_keys",
            &[
                (I32, &cid_str),
                (I32, &parent_cid_str),
                (I32, &n_str),
                (PTR, &keys_global),
                (I32, &keys_len_str),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::inline_shape_descriptor_facts_exact;

    #[test]
    fn raw_inline_shape_stamp_requires_exact_descriptor_facts() {
        assert!(inline_shape_descriptor_facts_exact(Some(5), 5));
        assert!(!inline_shape_descriptor_facts_exact(Some(5), 8));
        assert!(!inline_shape_descriptor_facts_exact(None, 5));
    }
}
