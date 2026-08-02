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
