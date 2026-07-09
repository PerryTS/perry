use perry_types::Type;

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
