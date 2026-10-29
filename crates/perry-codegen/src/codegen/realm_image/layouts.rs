//! The lazy image span contains descriptor values in static-row order. Direct
//! aliases give runtime births stable link names without retaining a catalog.
use super::*;
use crate::layout_desc::{desc_tail, key_off, DESC_TY, KEY_TY};

pub(crate) struct LayoutKey {
    pub(crate) atom: String,
    pub(crate) hash: u64,
    pub(crate) image: ImageKeySlot,
}

pub(crate) enum Prototype {
    Object,
    Null,
    Serial(u64),
    Image { serial: u64, cell: String },
}

pub(crate) struct LayoutFacts {
    pub(crate) symbol: String,
    pub(crate) word: u32,
    pub(crate) family: u32,
    pub(crate) variant: u16,
    pub(crate) kind: perry_abi::ShapeObjectKind,
    pub(crate) integrity: u8,
    pub(crate) slot_count: u32,
    pub(crate) rep: u64,
    pub(crate) prototype: Prototype,
    pub(crate) keys: Vec<LayoutKey>,
    pub(crate) class_id: u32,
    pub(crate) parent_class_id: u32,
}

/// Convert selected runtime births through the same descriptor/list emitter
/// as realm receivers. The whole-image planner owns row assignment; this
/// projection preserves the source's physical slot permutation and target.
pub(crate) fn runtime_layouts(
    module: &mut LlModule,
    plan: &crate::realm_plan::RealmPlan,
    atoms: &super::atoms::RealmAtoms,
    mut word: impl FnMut(&str, &perry_hir::realm_manifest::RuntimeLayoutVariant) -> Result<u32, String>,
) -> Result<Vec<LayoutFacts>, String> {
    use perry_hir::realm_manifest::RuntimeLayoutPrototype;
    let mut result = Vec::new();
    for (name, layout) in &plan.runtime_layouts {
        let source = layout.for_target(&module.target_triple)?;
        let kind = [
            perry_abi::ShapeObjectKind::Ordinary,
            perry_abi::ShapeObjectKind::OrdinaryUnmarked,
            perry_abi::ShapeObjectKind::CallableOrdinary,
        ]
        .into_iter()
        .find(|kind| kind.code() == u64::from(source.object_kind))
        .ok_or_else(|| format!("runtime layout {name} has unsupported receiver kind"))?;
        let prototype = match source.prototype {
            RuntimeLayoutPrototype::Object => Prototype::Object,
            RuntimeLayoutPrototype::Null => Prototype::Null,
            RuntimeLayoutPrototype::Serial { serial } => Prototype::Serial(serial),
        };
        let keys = source
            .keys
            .iter()
            .map(|key| atoms.layout_key(module, &key.atom.realm_value()?, key.slot, key.attributes))
            .collect::<Result<Vec<_>, String>>()?;
        result.push(LayoutFacts {
            symbol: layout.symbol.clone(),
            word: word(name, source)?,
            family: source.family,
            variant: source.variant,
            kind,
            integrity: source.integrity,
            slot_count: u32::try_from(keys.len())
                .map_err(|_| "runtime layout slot count overflow")?,
            rep: source.rep,
            prototype,
            keys,
            class_id: source.class_id,
            parent_class_id: source.parent_class_id,
        });
    }
    Ok(result)
}

/// Emit the one descriptor span selected by the whole-image ordinal plan.
/// Every row owns its keys and list; aliases add no pointer cells to the image.
pub(crate) fn emit_layouts(
    module: &mut LlModule,
    first: u32,
    layouts: &[LayoutFacts],
) -> Result<Vec<TagFixup>, String> {
    if layouts.is_empty() {
        return Ok(Vec::new());
    }
    let mut descriptors = Vec::new();
    let mut aliases = Vec::new();
    let mut fixups = Vec::new();
    for (index, layout) in layouts.iter().enumerate() {
        let row = first
            .checked_add(u32::try_from(index).map_err(|_| "too many realm layouts")?)
            .ok_or("realm row overflow")?;
        if perry_abi::LayoutWord(layout.word).row().0 != row {
            return Err(format!(
                "realm descriptor {} is not at its assigned row {row}",
                layout.symbol
            ));
        }
        let symbol = llvm_identifier(&layout.symbol);
        let base = format!("perry_lazy_layout_{row}");
        let key_count = layout.keys.len();
        let mut seen = vec![false; key_count];
        for key in &layout.keys {
            let slot = key.image.slot as usize;
            if slot >= key_count || slot >= layout.slot_count as usize || seen[slot] {
                return Err(format!(
                    "realm descriptor {} has invalid key slots",
                    layout.symbol
                ));
            }
            seen[slot] = true;
        }
        let keys = layout
            .keys
            .iter()
            .map(|key| {
                format!(
                    "{KEY_TY} {{ ptr @{}, i32 {}, i8 {}, i8 0, i16 {} }}",
                    llvm_identifier(&key.atom),
                    key.image.slot,
                    key.image.attrs as i8,
                    key_off(
                        &module.target_triple,
                        crate::target_layout::HeaderForm::Meta,
                        key.image.slot
                    ),
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        module.add_raw_global(format!(
            "@{base}_keys = private constant [{key_count} x {KEY_TY}] [{keys}], align 8"
        ));
        let capacity = (key_count * 2).next_power_of_two();
        let mut buckets = vec![u32::MAX; capacity];
        for (position, key) in layout.keys.iter().enumerate() {
            let mut bucket = key.hash as usize & (capacity - 1);
            while buckets[bucket] != u32::MAX {
                bucket = (bucket + 1) & (capacity - 1);
            }
            buckets[bucket] = position as u32;
        }
        module.add_raw_global(format!(
            "@{base}_index = private constant [{capacity} x i32] [{}], align 4",
            buckets
                .iter()
                .map(|i| format!("i32 {}", *i as i32))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        let image_keys = layout
            .keys
            .iter()
            .map(|key| ImageKeySlot {
                value: key.image.value.clone(),
                string_hash: key.image.string_hash,
                attrs: key.image.attrs,
                slot: key.image.slot,
            })
            .collect::<Vec<_>>();
        let (list, tags) = emit_image_key_list(module, &format!("{base}_list"), &image_keys);
        fixups.extend(tags);
        let prototype = match &layout.prototype {
            Prototype::Object => "null".into(),
            Prototype::Null => "inttoptr (i64 1 to ptr)".into(),
            Prototype::Serial(serial) | Prototype::Image { serial, .. } => {
                if *serial == 0 {
                    return Err(format!(
                        "realm descriptor {} lacks its prototype identity",
                        layout.symbol
                    ));
                }
                let pointer = match &layout.prototype {
                    Prototype::Image { cell, .. } => user_address(&llvm_identifier(cell)),
                    _ => "null".into(),
                };
                module.add_raw_global(format!(
                    "@{base}_prototype = private constant {{ i64, ptr }} {{ i64 {}, ptr {pointer} }}, align 8",
                    crate::nanbox::i64_literal(*serial)
                ));
                format!("@{base}_prototype")
            }
        };
        descriptors.push(format!(
            "{DESC_TY} {{ i32 {}, i16 {}, i8 {}, i8 {}, i32 {}, i32 {}, i64 {}, ptr {prototype}, ptr @{base}_keys, i32 {key_count}, i32 {}, ptr @{base}_index, i32 {}, i32 {}, ptr null, i32 0, i32 0, ptr {list}, {} }}",
            layout.family, layout.variant, layout.kind.code(), layout.integrity,
            layout.slot_count, layout.word as i32, crate::nanbox::i64_literal(layout.rep),
            capacity - 1, layout.class_id, layout.parent_class_id,
            desc_tail(&module.target_triple, crate::target_layout::HeaderForm::Meta, layout.slot_count),
        ));
        aliases.push(format!(
            "@{symbol} = alias {DESC_TY}, ptr getelementptr ([{} x {DESC_TY}], ptr @perry_lazy_layouts, i32 0, i32 {index})",
            layouts.len(),
        ));
    }
    let section = if module.target_triple.contains("apple") {
        "__DATA,__perry_layouts"
    } else if module.target_triple.contains("windows") {
        ".perrylay$m"
    } else {
        "perry_layouts"
    };
    module.add_raw_global(format!(
        "@perry_lazy_layouts = global [{} x {DESC_TY}] [{}], section \"{section}\", align 8",
        layouts.len(),
        descriptors.join(", "),
    ));
    for alias in aliases {
        module.add_raw_global(alias);
    }
    Ok(fixups)
}

#[cfg(test)]
mod tests;
