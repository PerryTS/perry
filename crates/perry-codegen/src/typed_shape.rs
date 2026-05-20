use perry_types::Type;

pub(crate) fn type_is_pointer_bearing(ty: &Type) -> bool {
    match ty {
        Type::String
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

pub(crate) fn trim_mask_words(mut words: Vec<u64>) -> Vec<u64> {
    while words.last().copied() == Some(0) {
        words.pop();
    }
    words
}

pub(crate) fn mask_words_for_fields<'a, I>(fields: I) -> Vec<u64>
where
    I: IntoIterator<Item = &'a perry_hir::ClassField>,
{
    let mut words = Vec::new();
    let mut slot = 0usize;
    for field in fields {
        if field.key_expr.is_some() {
            continue;
        }
        if type_is_pointer_bearing(&field.ty) {
            let word = slot / 64;
            if words.len() <= word {
                words.resize(word + 1, 0);
            }
            words[word] |= 1u64 << (slot % 64);
        }
        slot += 1;
    }
    trim_mask_words(words)
}

pub(crate) fn class_typed_layout(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    class_name: &str,
) -> (u32, Vec<u64>) {
    let Some(class) = classes.get(class_name).copied() else {
        return (0, Vec::new());
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

    let slot_count = chain
        .iter()
        .flat_map(|class| class.fields.iter())
        .filter(|field| field.key_expr.is_none())
        .count() as u32;
    let mask_words = mask_words_for_fields(chain.iter().flat_map(|class| class.fields.iter()));
    (slot_count, mask_words)
}

pub(crate) fn mask_global_name_from_keys_global(keys_global_name: &str) -> String {
    keys_global_name
        .strip_prefix("perry_class_keys_")
        .map(|suffix| format!("perry_typed_shape_mask_{}", suffix))
        .unwrap_or_else(|| format!("perry_typed_shape_mask_{}", keys_global_name))
}
