//! Layout descriptors the compiler emits for the realm. Realm receivers (PR B)
//! take the lazy image span, in static-row order, with direct aliases. Runtime
//! layouts in R are standalone descriptors under their own direct symbols,
//! carrying their fixed native words (DESIGN §3.2 item 9).
use super::*;
use crate::layout_desc::{desc_tail_for_family, key_off, ATOM_TY, DESC_TY, KEY_TY};

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
    pub(crate) realm_family: u8,
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

/// How a runtime layout's symbol keys are named.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolKeys {
    /// The realm's image symbol cells (the realm owns the symbols).
    Realm,
    /// The runtime's own symbol atoms: the well-known atoms by index and
    /// node's library symbols by number. The image list holds `undefined` in
    /// a symbol key's slot; the first install (`image_keys::prepare`) stores
    /// the runtime's symbol there, as for every process-lifetime symbol.
    Runtime,
}

/// Convert selected runtime births through the same descriptor/list emitter
/// as realm receivers. The whole-image planner owns row assignment; this
/// projection preserves the source's physical slot permutation and target.
pub(crate) fn runtime_layouts(
    module: &mut LlModule,
    plan: &crate::realm_plan::RealmPlan,
    atoms: &super::atoms::RealmAtoms,
    symbols: SymbolKeys,
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
            .map(|key| {
                if symbols == SymbolKeys::Runtime && key.atom.kind != perry_abi::ATOM_KIND_STRING {
                    return runtime_symbol_key(module, &key.atom, key.slot, key.attributes);
                }
                atoms.layout_key(module, &key.atom.realm_value()?, key.slot, key.attributes)
            })
            .collect::<Result<Vec<_>, String>>()?;
        result.push(LayoutFacts {
            symbol: layout.symbol.clone(),
            word: word(name, source)?,
            family: source.family,
            realm_family: 0,
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

/// A symbol key of a runtime layout, named by the runtime's own atom: the
/// runtime binds a well-known atom by its index and one of node's library
/// symbols by the atom's address, so the key is the runtime's symbol.
fn runtime_symbol_key(
    module: &mut LlModule,
    atom: &perry_hir::realm_manifest::RuntimeLayoutAtom,
    slot: u32,
    attrs: u8,
) -> Result<LayoutKey, String> {
    let (name, hash) = match atom.kind {
        perry_abi::ATOM_KIND_WELL_KNOWN_SYMBOL => {
            let spelling = perry_abi::WELL_KNOWN_SYMBOLS
                .get(atom.index as usize)
                .ok_or("unknown well-known layout symbol")?;
            let name = format!("perry_atom_well_known_{spelling}");
            let hash = perry_abi::well_known_symbol_atom_hash(atom.index);
            // The runtime defines the same weak_odr atom; one survives.
            let definition = format!("@{name} = ");
            if !module
                .global_definition_lines()
                .any(|line| line.starts_with(&definition))
            {
                let section = crate::layout_desc::section(
                    &module.target_triple,
                    "perry_atoms",
                    "__DATA,__perry_atoms",
                    ".perryatm$m",
                );
                let storage = crate::layout_desc::atom_storage(&module.target_triple);
                module.add_coalesced_global(&name, format!(
                    "@{name} = weak_odr {storage} {ATOM_TY} {{ i64 {}, ptr null, i8 {}, [3 x i8] zeroinitializer, i32 {} }}, section \"{section}\", align 8",
                    hash as i64, perry_abi::ATOM_KIND_WELL_KNOWN_SYMBOL, atom.index,
                ));
            }
            (name, hash)
        }
        perry_abi::ATOM_KIND_PROGRAM_SYMBOL => {
            let name = format!("perry_runtime_symbol_atom_{}", atom.index);
            module.add_external_declaration(format!("@{name} = external global {ATOM_TY}"));
            (name, perry_abi::runtime_symbol_atom_hash(atom.index))
        }
        _ => return Err("unsupported runtime layout symbol atom".into()),
    };
    Ok(LayoutKey {
        atom: name,
        hash,
        image: ImageKeySlot {
            value: Slot::Bits(crate::nanbox::TAG_UNDEFINED),
            string_hash: None,
            attrs,
            slot,
        },
    })
}

/// The descriptor initializer of `layout`, with its keys, index, prototype
/// and key list emitted as private globals named from `base`. `owner` is the
/// global that holds the descriptor (its own global for a runtime layout, the
/// span for lazy layouts): the key list lives and dies with it in the link.
fn descriptor(
    module: &mut LlModule,
    base: &str,
    layout: &LayoutFacts,
    owner: &str,
) -> Result<(String, Vec<TagFixup>), String> {
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
    let (list, fixups) = {
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
        emit_image_key_list(module, &format!("{base}_list"), &image_keys, Some(owner))
    };
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
    // DESIGN §3.2 item 2: the descriptor's gc plan, derived as for program
    // descriptors.
    let gc_plan = perry_abi::layout_gc_plan(layout.slot_count, layout.rep);
    Ok((
        format!(
            "{DESC_TY} {{ i32 {}, i16 {}, i8 {}, i8 {}, i32 {}, i32 {}, i64 {}, ptr {prototype}, ptr @{base}_keys, i32 {key_count}, i32 {}, ptr @{base}_index, i32 {}, i32 {}, ptr null, i32 0, i32 {}, ptr {list}, {} }}",
            layout.family, layout.variant, layout.kind.code(), layout.integrity,
            layout.slot_count, layout.word as i32, crate::nanbox::i64_literal(layout.rep),
            capacity - 1, layout.class_id, layout.parent_class_id, gc_plan as i32,
            desc_tail_for_family(&module.target_triple, crate::target_layout::HeaderForm::Meta, layout.slot_count, layout.realm_family),
        ),
        fixups,
    ))
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
    // The span is defined before the key lists it names: on ELF each list is
    // tied to it (`ArrayCell::owner`).
    let mark = module.global_mark();
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
        let (initializer, tags) = descriptor(
            module,
            &format!("perry_lazy_layout_{row}"),
            layout,
            "perry_lazy_layouts",
        )?;
        fixups.extend(tags);
        descriptors.push(initializer);
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
    module.insert_raw_global(
        mark,
        format!(
            "@perry_lazy_layouts = global [{} x {DESC_TY}] [{}], section \"{section}\", align 8",
            layouts.len(),
            descriptors.join(", "),
        ),
    );
    for alias in aliases {
        module.add_raw_global(alias);
    }
    Ok(fixups)
}

/// Emit each runtime layout as its own descriptor under its direct symbol,
/// outside every positional section: a birth names the symbol, never a
/// position (DESIGN §3.2 item 9). Its word is the layout's fixed native row.
pub(crate) fn emit_runtime_descriptors(
    module: &mut LlModule,
    layouts: &[LayoutFacts],
) -> Result<Vec<TagFixup>, String> {
    let mut fixups = Vec::new();
    for layout in layouts {
        if perry_abi::LayoutWord(layout.word).row().0 >= perry_abi::NATIVE_LAYOUT_ROWS {
            return Err(format!(
                "runtime descriptor {} does not carry a native row",
                layout.symbol
            ));
        }
        let symbol = llvm_identifier(&layout.symbol);
        // The descriptor is defined before its cells: on ELF a list names it
        // as its linked-to section, and the assembler of perry's textual `.s`
        // needs that section to exist when the list's section is opened.
        let mark = module.global_mark();
        let (initializer, tags) = descriptor(module, &symbol, layout, &symbol)?;
        fixups.extend(tags);
        module.insert_raw_global(mark, format!("@{symbol} = global {initializer}, align 8"));
    }
    Ok(fixups)
}

/// The fixed native word of runtime layout `name` (`%Layout:<NAME>%`): its
/// ABI row, with the prototype code the runtime's declaration carries.
pub(crate) fn native_runtime_word(
    name: &str,
    variant: &perry_hir::realm_manifest::RuntimeLayoutVariant,
) -> Result<u32, String> {
    use perry_hir::realm_manifest::RuntimeLayoutPrototype;
    let short = name
        .strip_prefix("%Layout:")
        .and_then(|n| n.strip_suffix('%'))
        .ok_or_else(|| format!("{name} is not a runtime layout name"))?;
    if !perry_abi::NATIVE_LAYOUT_DESCRIPTORS.contains(&short)
        || perry_abi::NATIVE_LAYOUT_RESERVED.contains(&short)
    {
        return Err(format!("{short} has no native layout row"));
    }
    let code = match variant.prototype {
        RuntimeLayoutPrototype::Object => 0,
        RuntimeLayoutPrototype::Serial { .. } => 1,
        RuntimeLayoutPrototype::Null => 2,
    };
    Ok(perry_abi::native_layout_word(short, code))
}

/// WRT3p-A1: the descriptors and key lists of the runtime layouts R reaches,
/// for the entry module's `main`. Nothing is emitted for a layout outside R.
/// The realm's own cells are PR B's; until then the runtime owns the symbols,
/// so symbol keys name the runtime's atoms.
pub(crate) fn emit_program_runtime_layouts(
    module: &mut LlModule,
    reach: &perry_hir::realm_reach::RealmReach,
) -> Result<Vec<TagFixup>, String> {
    let plan = crate::realm_plan::RealmPlan::from_reach(reach)?;
    if plan.runtime_layouts.is_empty() {
        return Ok(Vec::new());
    }
    // The string keys' atoms and image strings only: no realm cell and no
    // realm symbol cell is part of this emission.
    let mut string_layouts = plan.runtime_layouts.clone();
    for layout in string_layouts.values_mut() {
        for variant in &mut layout.variants {
            variant
                .keys
                .retain(|key| key.atom.kind == perry_abi::ATOM_KIND_STRING);
        }
    }
    let strings = crate::realm_plan::RealmPlan {
        cells: Default::default(),
        providers: Default::default(),
        native_namespaces: Default::default(),
        runtime_layouts: string_layouts,
        atoms: Default::default(),
        key_atoms: Default::default(),
        symbols: Default::default(),
    };
    let atoms = super::atoms::RealmAtoms::emit(module, &strings)?;
    let layouts = runtime_layouts(
        module,
        &plan,
        &atoms,
        SymbolKeys::Runtime,
        native_runtime_word,
    )?;
    emit_runtime_descriptors(module, &layouts)
}

#[cfg(test)]
mod tests;
