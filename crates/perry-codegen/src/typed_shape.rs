use perry_hir::types::Type;

/// ★ Can a value of this type be a heap reference the collector must see?
///
/// **This is the single definition of "pointer" for the whole codegen crate**,
/// and every consumer of that question routes here:
/// [`crate::collectors::pointer_locals::is_definitely_non_pointer_type`] (which
/// is its exact negation, and through it the shadow-slot assignment pass and
/// `collectors/ptr_shape_returns.rs`), the typed-shape pointer masks below, and
/// `expr/object_literal.rs`.
///
/// It is an **exhaustive `match`, deliberately**, not a `matches!` list. A
/// `matches!` list silently defaults a newly added `Type` variant to one answer,
/// and here the two answers are not symmetric: defaulting to "not a pointer"
/// means a local with **no shadow-stack slot**, i.e. a heap object the precise
/// moving-GC root scan cannot see. That is a use-after-move / premature sweep,
/// not a missed optimisation. The exhaustive match makes the compiler ask.
///
/// Per-variant notes, kept here because this is the only copy:
///
/// * `Symbol` — **a pointer.** `js_symbol_new` returns `POINTER_TAG`-boxed
///   storage from `alloc_symbol`, which is
///   `gc_malloc(size_of::<SymbolHeader>(), GC_TYPE_STRING)`. Note *which* way
///   it is dangerous: `gc_malloc` is the SYSTEM allocator with a `GcHeader` in
///   front, not an arena allocation, so the copying minor cannot relocate a
///   symbol — it can `dealloc` it (`sweep_malloc_objects`, reached from the
///   copying minor whenever `copied_minor_malloc_sweep_due`). Under #7235's
///   taxonomy a symbol is RECLAIMABLE and not MOVABLE, and nothing else holds a
///   fresh one: `alloc_symbol`'s own comment says it is kept alive "through the
///   SYMBOL_REGISTRY … or NOT AT ALL", and `SYMBOL_POINTERS` is visited with
///   `visit_metadata_usize_slot`, which rewrites without marking. So an
///   unrooted `Symbol` local is a premature FREE (#7230's class), not a stale
///   address. It was listed as a non-pointer by
///   `is_definitely_non_pointer_type` (#7236) while this function already
///   answered `true` — the exact one-variant drift the doc comment over there
///   warned about, which is why there is now one copy.
/// * `StringLiteral` — a string-LITERAL type (`"foo"`, or a `"a" | "b"`
///   discriminant union member) is an ordinary heap `String` at runtime.
/// * `TypeVar` — an unresolved generic type parameter (`T`) can bind to any
///   heap type; treat it as a pointer (fail-safe).
/// * `Generic` — a generic instantiation (`Map<K,V>`, `Set<T>`, `WeakMap`,
///   `Box<T>`, `Array<T>`, a user generic class, …) is always a heap-reference
///   type. Without this a `Map`/`Set`-typed local got no shadow-stack slot and
///   was reaped while live (#7019, crash: "grown Map must retain its
///   side-allocation owner record"); the non-moving default GC hid it behind
///   its conservative C-stack scan.
/// * `Number` / `Int32` / `Boolean` / `Null` / `Void` — NaN-boxed immediates
///   (a raw `f64`, `INT32_TAG`, `TAG_TRUE`/`TAG_FALSE`, `TAG_NULL`,
///   `TAG_UNDEFINED`): no allocator produces one, so there is nothing to root.
///   `Never` has no value at all. These six are the complete non-pointer set.
/// * `Union` — pointer-bearing if ANY member is, so the negation is "every
///   member is a non-pointer", which is what the root-slot decision needs.
///
/// Over-answering `true` is harmless: the GC decode rejects any slot value that
/// is not a live heap pointer, so a spurious root costs one slot, never
/// correctness. Under-answering is #7019 / #7236.
pub(crate) fn type_is_pointer_bearing(ty: &Type) -> bool {
    match ty {
        Type::String
        | Type::StringLiteral(_)
        | Type::BigInt
        | Type::Symbol
        | Type::Array(_)
        | Type::Tuple(_)
        | Type::Object(_)
        | Type::Function(_)
        | Type::Promise(_)
        | Type::Named(_)
        | Type::Generic { .. }
        | Type::Any
        | Type::Unknown
        | Type::TypeVar(_) => true,
        Type::Union(variants) => variants.iter().any(type_is_pointer_bearing),
        Type::Void | Type::Null | Type::Boolean | Type::Number | Type::Int32 | Type::Never => false,
    }
}

pub(crate) fn type_is_raw_f64_candidate(ty: &Type) -> bool {
    matches!(ty, Type::Number)
}

/// #7510: may this class's canonical layout be declared at **allocation**,
/// before its constructor runs, rather than validated after it?
///
/// The motivating defect is #7512: `js_gc_init_typed_shape_layout` is emitted
/// after the constructor call, so no raw-f64 class-field store *inside* a
/// constructor can pass its `GC_OBJ_TYPED_LAYOUT_INTACT` guard, and every one
/// falls back to `js_put_value_set`. Declaring the fields `number` is what
/// makes the class slower — more type information selects a representation
/// whose guard the construction path has made unsatisfiable.
///
/// Moving the existing call earlier does not work: it validates that each
/// raw-f64 slot holds a plain double, and a fresh slot holds `TAG_UNDEFINED`
/// (tag `0x7FFC`, inside `layout_raw_f64_bits`' reject range), so an early call
/// downgrades every instance. `js_gc_declare_typed_shape_layout` skips that
/// validation, which shifts the burden of proof here.
///
/// Two obligations, and both are discharged by conditions, not by hope:
///
/// 1. **No read may observe a raw-f64 slot before its first write** — it would
///    read `undefined`'s NaN-box bits as a double and see a NaN. `prologue`
///    is #7486's `ctor_prologue_param_assigned_fields`: the maximal leading run
///    of `this.<f> = <plain param>` statements, non-empty only for a class with
///    no heritage, no field initializers or computed keys, no decorators, plain
///    parameters, and no setter shadowing an assigned field. A `LocalGet` of a
///    plain parameter cannot throw, allocate, or observe `this`, so every field
///    it assigns is written before ANY other effect of the constructor. We
///    require **every** raw-f64 field to be in that set — one field assigned
///    later would still be exposed.
///
/// 2. **The collector's view must be true at birth.** The declared state is
///    `GC_LAYOUT_POINTER_FREE` for an empty pointer mask and `SIDE_MASK`
///    otherwise, and in both cases the collector is handed slots that still
///    hold the allocator's fill. That fill is `TAG_UNDEFINED` on **every**
///    allocation path a `new` site can take — `js_object_alloc_class_inline_keys`
///    writes `max(field_count, INLINE_SLOT_FLOOR)` slots (`object/alloc.rs`,
///    #4717) and codegen's inline bump path writes the same range with the same
///    constant (`lower_call/new_alloc.rs`) — so a pointer-masked slot the tracer
///    visits before its first write yields `undefined`, which
///    `mark_field_into_worklist` rejects at its tag check. A raw-f64-masked slot
///    is not visited at all. Neither can strand a child, because neither holds
///    one yet.
///
///    This is the one obligation the original #7510 rule discharged by *avoiding*
///    it (pointer mask required empty) rather than by proving it, which cost
///    every pointer-bearing class its at-allocation declaration — and with it
///    every store in its constructor, since the post-constructor install arrives
///    after them all. `tree_wide`'s eight `number` fields were on the by-name
///    fallback for exactly this reason: two `Tree | null` siblings.
///
/// Nothing rests on the *values* being numbers. A constructor that stores a
/// string into a `number`-declared field is rejected by the store guard
/// (`is_plain_number_bits`, and the inline path's finite-exponent test), falls
/// back to the boxed setter, and downgrades the descriptor through
/// `layout_note_slot` — the same path any post-install contradiction takes.
/// [`class_layout_declarable_at_allocation`] for a whole inheritance chain.
///
/// The single-class rule refuses every class with heritage, which is #7512
/// one level up: a `Rect extends Shape extends Node2D` instance never gets an
/// at-allocation declaration, so every raw-f64 store in *every* constructor on
/// its chain — `Node2D`'s own `this.x = x` included, though `Node2D` itself
/// extends nothing — misses `GC_OBJ_TYPED_LAYOUT_INTACT` and falls back to
/// `js_put_value_set`. Counted on `shapes.ts`: 528 000 by-name field stores per
/// run. A two-class probe measures **2.0x** against the hand-flattened class.
///
/// `chain` is `chain_prologue_assigned_fields`' root → leaf answer, which is
/// `None` unless every class on the chain is individually analysable. The
/// obligations are the single-class ones, restated over the chain:
///
/// 1. **Every raw-f64 field ANYWHERE on the chain is prologue-assigned** — by
///    its own class, since that is the only constructor that writes it. One
///    uncovered field would be read as a double while it still holds
///    `undefined`'s NaN-box bits, yielding `NaN` instead of `undefined`.
/// 2. **Nothing during construction can read a field before its write.** Each
///    class's prologue RHS and `super()` arguments are `This`-free by
///    `prologue_rhs_cannot_observe_this`, and everything after a class's
///    prologue run is `This`-free by `stmt_is_this_free_expr` — which matters
///    precisely because a *non-leaf* constructor's trailing statements run
///    before the leaf assigns its own fields.
/// 3. **The collector's view is true at birth** — unchanged from the
///    single-class case: every allocation path prefills `TAG_UNDEFINED`, a
///    pointer-masked slot holding it is rejected at `mark_field_into_worklist`'s
///    tag check, and a raw-f64-masked slot is not visited at all.
pub(crate) fn class_chain_layout_declarable_at_allocation(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    chain: &[(String, std::collections::HashSet<String>)],
) -> bool {
    let mut worth_declaring = false;
    for (class_name, prologue) in chain {
        let Some(class) = classes.get(class_name).copied() else {
            return false;
        };
        for field in &class.fields {
            if field.key_expr.is_some() {
                continue;
            }
            if type_is_pointer_bearing(&field.ty) {
                worth_declaring = true;
            }
            if type_is_raw_f64_candidate(&field.ty) {
                worth_declaring = true;
                if !prologue.contains(&field.name) {
                    return false;
                }
            }
        }
    }
    worth_declaring
}

pub(crate) fn class_layout_declarable_at_allocation(
    class: &perry_hir::Class,
    prologue: &std::collections::HashSet<String>,
) -> bool {
    if prologue.is_empty() {
        return false;
    }
    // The declaration costs one call per construction, so it must buy at least
    // one mask bit. A class whose every field is `boolean` declares two empty
    // masks: the boxed store arm does not read the intact bit at all
    // (`require_raw_f64 = false`), and the collector's view is already
    // `POINTER_FREE` from `layout_init_pointer_free`. Nothing to unlock.
    let mut worth_declaring = false;
    for field in &class.fields {
        if field.key_expr.is_some() {
            continue;
        }
        if type_is_pointer_bearing(&field.ty) {
            worth_declaring = true;
        }
        // Obligation 1 applies to raw-f64 slots ONLY. A pointer-masked slot
        // read before its first write yields `undefined`, which is the correct
        // answer; a raw-f64 slot read before its first write reinterprets
        // `undefined`'s NaN-box bits as a double and yields NaN. So only the
        // latter needs the prologue's write-before-anything-else guarantee.
        //
        // It also covers the field-init phase's own `undefined` write: that
        // write lands in a raw-f64-masked slot, fails `layout_raw_f64_bits`,
        // and would downgrade the descriptor on the spot — but a
        // prologue-assigned field has that write ELIDED
        // (`ctor_prologue_param_assigned_fields`, the same set), so it never
        // happens. The two consumers of `prologue` have to agree here, and
        // they agree because it is literally one set.
        if type_is_raw_f64_candidate(&field.ty) {
            worth_declaring = true;
            if !prologue.contains(&field.name) {
                return false;
            }
        }
    }
    worth_declaring
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TypedShapeLayout {
    pub(crate) slot_count: u32,
    pub(crate) raw_f64_mask_words: Vec<u64>,
    pub(crate) pointer_mask_words: Vec<u64>,
}

pub(crate) fn trim_mask_words(mut words: Vec<u64>) -> Vec<u64> {
    while words.last().copied() == Some(0) {
        words.pop();
    }
    words
}

pub(crate) fn class_typed_layout(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    class_name: &str,
) -> TypedShapeLayout {
    let Some(class) = classes.get(class_name).copied() else {
        return TypedShapeLayout::default();
    };
    let mut chain: Vec<&perry_hir::Class> = Vec::new();
    let mut cur = Some(class);
    let mut depth = 0usize;
    while let Some(c) = cur {
        chain.push(c);
        depth += 1;
        if depth > 64 {
            break;
        }
        cur = c
            .extends_name
            .as_deref()
            .and_then(|parent| classes.get(parent).copied());
    }
    chain.reverse();

    typed_layout_from_fields(chain.iter().flat_map(|class| class.fields.iter()))
}

/// Issue #26 / #321 (refs #5094): typed layout from an authoritative,
/// source-prefix-disambiguated root→leaf chain (`class_init_chains`). The
/// name-keyed walk in [`class_typed_layout`] mis-resolves same-named
/// cross-module parents (effect's `Type` in SchemaAST.ts vs ParseResult.ts),
/// which misaligns every mask bit after the wrong parent's field count. The
/// GC scanner reads these masks per slot, so a misaligned mask is only kept
/// from corrupting memory by the install-time backstop in
/// `js_gc_init_typed_shape_layout` (each raw-f64 slot is validated to hold a
/// plain double before the descriptor is promoted) — which also means
/// dup-named classes silently never get a typed descriptor, so the #5093
/// class-field fast path never engages for them. The chain is built in
/// `compile_module` by the SAME walk that emits the packed-keys global and
/// field count, so masks derived from it are consistent with the slot layout
/// instances actually get.
pub(crate) fn class_typed_layout_from_chain(
    chain: &[(String, Vec<perry_hir::ClassField>)],
) -> TypedShapeLayout {
    typed_layout_from_fields(chain.iter().flat_map(|(_, fields)| fields.iter()))
}

fn typed_layout_from_fields<'a>(
    fields: impl Iterator<Item = &'a perry_hir::ClassField>,
) -> TypedShapeLayout {
    let mut raw_f64_mask_words = Vec::new();
    let mut pointer_mask_words = Vec::new();
    let mut slot_count = 0u32;
    for field in fields {
        if field.key_expr.is_some() {
            continue;
        }
        let slot = slot_count as usize;
        if type_is_raw_f64_candidate(&field.ty) {
            let word = slot / 64;
            if raw_f64_mask_words.len() <= word {
                raw_f64_mask_words.resize(word + 1, 0);
            }
            raw_f64_mask_words[word] |= 1u64 << (slot % 64);
        }
        if type_is_pointer_bearing(&field.ty) {
            let word = slot / 64;
            if pointer_mask_words.len() <= word {
                pointer_mask_words.resize(word + 1, 0);
            }
            pointer_mask_words[word] |= 1u64 << (slot % 64);
        }
        slot_count += 1;
    }

    TypedShapeLayout {
        slot_count,
        raw_f64_mask_words: trim_mask_words(raw_f64_mask_words),
        pointer_mask_words: trim_mask_words(pointer_mask_words),
    }
}

/// Does `layout`'s **pointer** mask declare `slot`?
///
/// The masks are word-packed exactly as `typed_layout_from_fields` builds them
/// and as `js_gc_{init,declare}_typed_shape_layout` consumes them, so a `true`
/// here is the same bit the runtime's `TypedLayoutDescriptor::pointer_mask`
/// will carry for this shape.
pub(crate) fn layout_declares_pointer_slot(layout: &TypedShapeLayout, slot: u32) -> bool {
    let slot = slot as usize;
    if slot >= layout.slot_count as usize {
        return false;
    }
    let word = slot / 64;
    // A pointer-masked slot may not also be raw-f64-masked. `init_typed_shape_layout`
    // rejects an intersecting pair outright (`words_intersect` -> UNKNOWN), so a
    // shape that reaches an installed descriptor has disjoint masks — but this
    // predicate licenses eliding a store's layout note, so it re-establishes
    // disjointness locally rather than importing it.
    let raw_f64_here = layout
        .raw_f64_mask_words
        .get(word)
        .is_some_and(|w| w & (1u64 << (slot % 64)) != 0);
    if raw_f64_here {
        return false;
    }
    layout
        .pointer_mask_words
        .get(word)
        .is_some_and(|w| w & (1u64 << (slot % 64)) != 0)
}

pub(crate) fn mask_global_name_from_keys_global(keys_global_name: &str) -> String {
    keys_global_name
        .strip_prefix("perry_class_keys_")
        .map(|suffix| format!("perry_typed_shape_mask_{}", suffix))
        .unwrap_or_else(|| format!("perry_typed_shape_mask_{}", keys_global_name))
}

pub(crate) fn raw_f64_mask_global_name_from_keys_global(keys_global_name: &str) -> String {
    keys_global_name
        .strip_prefix("perry_class_keys_")
        .map(|suffix| format!("perry_typed_shape_raw_f64_mask_{}", suffix))
        .unwrap_or_else(|| format!("perry_typed_shape_raw_f64_mask_{}", keys_global_name))
}

/// The module-global ShapeId paired with one canonical class keys array.
///
/// Keeping the name derived from the already-unique keys global means aliases
/// and sanitized-name collisions necessarily share the same pair. The id is
/// minted once, immediately after `js_build_class_keys_array`, and loaded by
/// every compiled construction path so class instances arrive birth-stamped
/// instead of waiting for their first by-name lookup (#6759 C3 rung 2).
pub(crate) fn shape_id_global_name_from_keys_global(keys_global_name: &str) -> String {
    keys_global_name
        .strip_prefix("perry_class_keys_")
        .map(|suffix| format!("perry_class_shape_id_{}", suffix))
        .unwrap_or_else(|| format!("perry_class_shape_id_{}", keys_global_name))
}

/// #8122: the per-class `<2 x i64>` header-image global paired with a class's
/// canonical keys global — `[packed GcHeader word | class_id | ShapeId << 32]`,
/// composed once at module init right after the ShapeId mint, so the inline
/// `new` path stores an instance's 16-byte header prefix with one vector
/// store instead of composing it per site (or per call, in a recursive
/// allocator).
pub(crate) fn header_image_global_name_from_keys_global(keys_global_name: &str) -> String {
    keys_global_name
        .strip_prefix("perry_class_keys_")
        .map(|suffix| format!("perry_class_header_image_{}", suffix))
        .unwrap_or_else(|| format!("perry_class_header_image_{}", keys_global_name))
}

/// Load the immutable ShapeId paired with a class's canonical keys global.
///
/// Cache it in a function-entry alloca: an opaque allocation/runtime call can
/// otherwise prevent LLVM from hoisting the module-global load out of a hot
/// loop. This scalar is not a GC root and needs no shadow-slot binding.
pub(crate) fn load_class_shape_id(
    ctx: &mut crate::expr::FnCtx<'_>,
    class_name: &str,
    keys_global_name: &str,
) -> String {
    let shape_slot = ensure_class_shape_slot(ctx, class_name, keys_global_name);
    ctx.block().load(crate::types::I32, &shape_slot)
}

/// The function-entry alloca that caches a class's ShapeId global (see
/// [`load_class_shape_id`]), created on first use. Split out (#8122) so the
/// inline `new` path can compose its per-function header image from the slot
/// without emitting a per-site load it does not need.
pub(crate) fn ensure_class_shape_slot(
    ctx: &mut crate::expr::FnCtx<'_>,
    class_name: &str,
    keys_global_name: &str,
) -> String {
    if let Some(slot) = ctx.class_shape_slots.get(class_name).cloned() {
        return slot;
    }
    let shape_global = shape_id_global_name_from_keys_global(keys_global_name);
    let slot = ctx
        .func
        .entry_init_load_global(&shape_global, crate::types::I32);
    ctx.class_shape_slots
        .insert(class_name.to_string(), slot.clone());
    slot
}
