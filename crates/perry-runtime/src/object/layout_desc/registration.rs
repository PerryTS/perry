use super::*;

/// The startup registrar of a runtime with no compiled entry (Rust tests).
#[cfg(test)]
pub(crate) fn register_static_layouts() {
    // SAFETY: a bound layout image names its program's table.
    register_static_layouts_in(unsafe { &*super::super::class_image::layout_image().program });
}

/// [`register_static_layouts`] for `program`, the bound image's table the
/// caller already holds (`js_shape_run_static_seed`).
#[inline(always)]
pub(crate) fn register_static_layouts_in(program: &crate::codegen_abi::PerryProgramTableV1) {
    // A lazy image span publishes at its rows' first touch, never here; a
    // program with one takes the walk around it, a program without walks
    // exactly as before.
    if program.lazy_count != 0 {
        return unsafe { register_around_lazy_span(program) };
    }
    unsafe {
        for (index, desc) in image_layouts().enumerate() {
            if !register_static_row(index, desc) {
                break;
            }
        }
    }
}

/// The startup walk of an image with a lazy span. The span is one contiguous
/// run of the section, so the walk goes around it and no row pays a test.
#[cold]
#[optimize(size)]
#[inline(never)]
unsafe fn register_around_lazy_span(program: &crate::codegen_abi::PerryProgramTableV1) {
    let (start, end) = layout_bounds();
    let (lazy_start, lazy_end) = super::lazy_image::section_span(program, start, end);
    let size = std::mem::size_of::<PerryLayoutDesc>();
    for (index, desc) in records::<PerryLayoutDesc>((start, lazy_start)).enumerate() {
        if !register_static_row(index, desc) {
            return;
        }
    }
    if lazy_end != end {
        // Rows after the span keep their section index.
        let after = (lazy_end - start) / size;
        for (index, desc) in records::<PerryLayoutDesc>((lazy_end, end)).enumerate() {
            if !register_static_row(after + index, desc) {
                return;
            }
        }
    }
}

/// One static row at startup; false stops the walk (no ids left).
#[inline(always)]
unsafe fn register_static_row(index: usize, desc: *mut PerryLayoutDesc) -> bool {
    {
        {
            if (*desc).family == SENTINEL_FAMILY || (*desc).keys.is_null() && (*desc).key_count != 0
            {
                return true;
            }
            if (*desc).layout_id == 0 {
                return true;
            }
            // WRT step 2: a runtime layout or a module namespace draws its id
            // and installs its shape at its first birth
            // ([`layout_word_for_birth`], [`register_namespace_layout`]);
            // startup installs the program's own layouts only. The id cell is
            // process-wide, so every agent still sees one id per layout.
            if (*desc).variant == crate::codegen_abi::LAYOUT_VARIANT_NAMESPACE
                || is_runtime_layout_word((*desc).layout_id)
            {
                return true;
            }
            let Some(id) = layout_id_of_index(index as u32) else {
                return false;
            };
            let _ = register_one(desc, id);
            true
        }
    }
}

/// Is `word` a runtime layout's (`runtime_layout!`: the ABI's native rows,
/// and in perry-runtime's tests the test band)? Generated code never names
/// such a descriptor, so every object carrying one is born by a runtime
/// funnel ([`layout_word_for_birth`]), which draws its id and installs its
/// shape at the first birth.
#[inline]
pub(super) fn is_runtime_layout_word(word: u32) -> bool {
    word & crate::codegen_abi::LAYOUT_TAG != 0
        && LayoutWord(word).row().0 < super::super::runtime_layouts::RUNTIME_LAYOUT_ROWS
}

/// The registered word of `desc` for a birth in this agent. A runtime
/// layout's shape is installed by its first birth here; every other
/// registered layout was installed at startup.
#[inline]
pub(crate) unsafe fn layout_word_for_birth(desc: *const PerryLayoutDesc) -> Option<u32> {
    let id = registered_layout_id(load_layout_id(desc))?;
    if is_runtime_layout_word(id)
        && super::super::shapes::shape_record_for_word(LayoutWord(id)).is_none()
    {
        install_runtime_layout(desc)?;
    }
    Some(id)
}

#[cold]
#[inline(never)]
unsafe fn install_runtime_layout(desc: *const PerryLayoutDesc) -> Option<()> {
    // A runtime layout is its own emitted symbol with no position, so it
    // requests no id: it draws one at its first install (§2.4a decision 1).
    let id = register_bound(desc, 0, &[], None);
    (id != 0 && super::super::shapes::shape_record_for_word(LayoutWord(id)).is_some()).then_some(())
}

#[cfg(any(test, debug_assertions))]
crate::perry_thread_local! {
    /// Runtime layouts whose shape this agent installed (debug and test
    /// builds only): startup installs none.
    pub(crate) static RUNTIME_LAYOUT_INSTALLS: std::cell::Cell<u32> =
        const { std::cell::Cell::new(0) };
}

/// What [`register_bound`] does with a descriptor that passed validation.
/// The installed layout of module namespace descriptor `desc`, installing
/// its key list and shape under the id startup reserved on the first birth
/// in this agent; `None` when this runtime cannot represent it.
pub(crate) unsafe fn register_namespace_layout(desc: *const PerryLayoutDesc) -> Option<u32> {
    let id = register_one(desc, 0);
    (id != 0).then_some(id)
}

/// The shape kind a descriptor's `kind` code declares, among the ordinary
/// kinds the registrar represents: `Ordinary`, and for a runtime prototype
/// whose stores must reach their funnel, `OrdinaryUnmarked` or
/// `CallableOrdinary`.
pub(super) fn declared_kind(code: u8) -> Option<super::super::shapes::ShapeObjectKind> {
    use super::super::shapes::ShapeObjectKind::{CallableOrdinary, Ordinary, OrdinaryUnmarked};
    [Ordinary, OrdinaryUnmarked, CallableOrdinary]
        .into_iter()
        .find(|k| k.code() == code as u64)
}

/// Register one descriptor under `requested`; the id it resolved to, or 0
/// when this runtime cannot represent it yet (a descriptor prototype, a
/// non-ordinary kind, slots that are not a permutation of the key positions,
/// a key that is not an atom). The key sequence is the enumeration order;
/// each key's slot is its storage.
pub(crate) unsafe fn register_one(desc: *const PerryLayoutDesc, requested: u32) -> u32 {
    register_one_bound(desc, requested, &[])
}

/// The module-local symbol keys (`ATOM_KIND_BINDING_SYMBOL`) of a
/// descriptor's key sequence.
pub(super) unsafe fn binding_symbol_keys(d: &PerryLayoutDesc) -> usize {
    if d.key_count == 0 {
        return 0;
    }
    std::slice::from_raw_parts(d.keys, d.key_count as usize)
        .iter()
        .filter(|k| (*k.atom).kind == crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL)
        .count()
}

/// The key word of key `key` of a descriptor: a string's, a symbol atom's
/// symbol, or for the `j`-th module-local symbol key, `bound[j]` (`j` counts
/// those keys before it, `next` the running count).
pub(crate) unsafe fn layout_key_word(key: &PerryLayoutKey, bound: &[u64], next: &mut usize) -> u64 {
    if (*key.atom).kind == crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL {
        let word = bound[*next];
        *next += 1;
        return word;
    }
    match atom_symbol(key.atom) {
        // The key word a symbol data property stores.
        Some(symbol) => crate::value::POINTER_TAG | symbol as u64,
        None => crate::value::JSValue::string_ptr((*key.atom).string.cast_mut()).bits(),
    }
}

/// [`register_one`] for a descriptor with module-local symbol keys: `bound`
/// holds their key words, in key order. Without them (startup) the id is
/// only reserved: the word names it, but no shape is installed until the
/// prototype's bind hands the symbols (`perry_bind_family_prototype`), so no
/// object carries the id before then.
pub(crate) unsafe fn register_one_bound(
    desc: *const PerryLayoutDesc,
    requested: u32,
    bound: &[u64],
) -> u32 {
    register_bound(desc, requested, bound, None)
}

/// An ordinary function's layout, with the actual prototype serial carried
/// by its ordinary shape record. The existing identity word roots its link.
pub(crate) unsafe fn register_function_layout(
    desc: *const PerryLayoutDesc,
    requested: u32,
    bound: &[u64],
    proto_id: u64,
) -> u32 {
    register_bound(desc, requested, bound, Some(proto_id))
}

/// A lazy image row at its first touch: its identity band follows its kind.
/// The realm image hands every lazy row its canonical key list as an image
/// cell (the emitter writes one for every realm layout) over string and
/// image symbol atoms only, so the registration reads
/// image data and installs: it allocates nothing on the GC heap and never
/// collects, and holds no suppression scope. A registered or module-local
/// symbol key, or a row without its image list, breaks the image contract:
/// the answer 0, which `publish_receiver` turns into a loud failure.
pub(super) unsafe fn register_lazy_row(desc: *const PerryLayoutDesc) -> u32 {
    // A receiver read during shape publication must not re-enter registration.
    #[cfg(any(test, debug_assertions))]
    let _entered = LazyRowEntry::enter();
    #[cfg(any(test, debug_assertions))]
    let _no_gc = super::NoGcAlloc::enter();
    let facts = match row_facts::<KEYS_LAZY>(desc, 0, &[], None) {
        Ok(facts) => facts,
        Err(id) => return id,
    };
    install_from_image_list(desc, &facts)
}

/// The registrar validates keys according to their descriptor source.
const KEYS_BOUND: u8 = 0;
const KEYS_LAZY: u8 = 1;

/// A lazy row's key: the realm image names every key of a receiver row as a
/// string or an image symbol cell (codegen `RealmAtoms::layout_key`); any
/// other kind breaks the image contract and is refused.
#[inline]
unsafe fn lazy_key_ok(key: &PerryLayoutKey) -> bool {
    let atom = &*key.atom;
    match atom.kind {
        crate::codegen_abi::ATOM_KIND_STRING => !atom.string.is_null(),
        crate::codegen_abi::ATOM_KIND_IMAGE_SYMBOL => {
            !atom.string.is_null()
                && crate::value::addr_class::realm::is_image_object(atom.string as usize)
        }
        _ => false,
    }
}

/// The eager registrar's key kinds.
#[inline]
unsafe fn bound_key_ok(key: &PerryLayoutKey, d: &PerryLayoutDesc) -> bool {
    match (*key.atom).kind {
        crate::codegen_abi::ATOM_KIND_STRING => !(*key.atom).string.is_null(),
        crate::codegen_abi::ATOM_KIND_WELL_KNOWN_SYMBOL => {
            ((*key.atom).index as usize) < crate::codegen_abi::WELL_KNOWN_SYMBOLS.len()
        }
        crate::codegen_abi::ATOM_KIND_IMAGE_SYMBOL => {
            !(*key.atom).string.is_null()
                && crate::value::addr_class::realm::is_image_object((*key.atom).string as usize)
        }
        // A symbol of node's own library the runtime binds
        // (`runtime_layouts::runtime_symbol_of_atom`).
        crate::codegen_abi::ATOM_KIND_PROGRAM_SYMBOL => {
            super::super::runtime_layouts::runtime_symbol_of_atom(key.atom).is_some()
        }
        // A registered symbol (`Symbol.for(desc)`) the program names.
        crate::codegen_abi::ATOM_KIND_REGISTERED_SYMBOL => !(*key.atom).string.is_null(),
        // A module-local symbol: a prototype image's, bound at its bind.
        crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL => {
            d.variant == PROTOTYPE_IMAGE_VARIANT
                || d.variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR
        }
        _ => false,
    }
}

/// Link a validated row's image key list (the cell the realm image emits
/// with every layout it holds) and install the row: reads image data only,
/// allocates nothing on the GC heap. 0 when the list is missing or does not
/// match the descriptor's keys.
unsafe fn install_from_image_list(
    desc: *const PerryLayoutDesc,
    facts: &RowFacts,
) -> u32 {
    let list_keys = if facts.keys.is_empty() {
        super::super::ObjectKeys::NONE
    } else {
        let cell = (*desc).image_keys;
        let proof = super::super::canonical_keys::SharedLayout::shape_cache_entry();
        if !super::super::image_keys::prepare_lazy(facts.keys, cell) {
            return 0;
        }
        let list = super::super::canonical_keys::static_seed::link_lazy_image_list(&proof, facts.keys, cell);
        match list {
            Some(list) => list.view(),
            None => return 0,
        }
    };
    install_row(desc, facts, list_keys)
}

#[cfg(any(test, debug_assertions))]
struct LazyRowEntry;
#[cfg(any(test, debug_assertions))]
std::thread_local! {
    static LAZY_ROW_ENTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
#[cfg(any(test, debug_assertions))]
impl LazyRowEntry {
    fn enter() -> Self {
        assert!(
            !LAZY_ROW_ENTERED.with(|e| e.replace(true)),
            "lazy-row registration re-entered itself"
        );
        LazyRowEntry
    }
}
#[cfg(any(test, debug_assertions))]
impl Drop for LazyRowEntry {
    fn drop(&mut self) {
        LAZY_ROW_ENTERED.with(|e| e.set(false));
    }
}

/// What registration validated of a descriptor, shared by the eager and the
/// lazy registrar (`row_facts`), consumed by `install_row`.
#[derive(Clone, Copy)]
struct RowFacts {
    keys: &'static [PerryLayoutKey],
    kind: super::super::shapes::ShapeObjectKind,
    function_summary: u8,
    function: bool,
    mapped: bool,
    existing: Option<u32>,
    class_family: bool,
    proto_id: u64,
    requested: u32,
    lazy: bool,
}

/// Validate a descriptor and derive its kind and prototype identity. `Err`
/// carries the registration's answer when nothing is left to install: 0
/// (refused), or the layout word (already installed, or only reserved).
// Specialize the key policy and lazy fact at the registrar call site.
#[inline(always)]
unsafe fn row_facts<const KEYS: u8>(
    desc: *const PerryLayoutDesc,
    requested: u32,
    bound: &[u64],
    function_proto: Option<u64>,
) -> Result<RowFacts, u32> {
    let lazy = KEYS == KEYS_LAZY;
    use std::sync::atomic::{AtomicU32, Ordering};
    let word = AtomicU32::from_ptr(std::ptr::addr_of!((*desc).shape_id).cast_mut());
    let function = (*desc).variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR;
    if function && function_proto.is_none() {
        return Err(0);
    }
    let existing = if function {
        None
    } else {
        Some(word.load(Ordering::Acquire)).filter(|&id| id != 0)
    };
    let local = binding_symbol_keys(&*desc);
    if let Some(id) = existing {
        if (local != 0 && bound.is_empty())
            || super::super::shapes::shape_descriptor_by_id(ShapeId(id)).is_some()
        {
            return Err((*desc).layout_id);
        }
    }
    if local != 0 && !bound.is_empty() && bound.len() != local {
        return Err(0);
    }
    let d = &*desc;
    let (kind, function_summary) = match declared_kind(d.kind) {
        Some(kind) => (kind, 0),
        None if !lazy => return Err(0),
        None => match super::lazy_image::receiver_kind(desc) {
            Some(kind) => (kind, crate::closure::shape::function_shape_summary(kind)),
            None => return Err(0),
        },
    };
    if existing.is_none() && !function && requested >= super::super::shapes::STATIC_SHAPE_ID_END {
        return Err(0);
    }
    let all_keys: &'static [PerryLayoutKey] = if d.key_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(d.keys, d.key_count as usize)
    };
    // An optional key (a class family's late key, case 2c, or a runtime
    // layout's optional member) is in the birth key list with
    // `key_attrs::ENTRY_OPTIONAL`: absent while its slot holds the hole
    // (`key_view`).
    let keys = all_keys;
    // Names, hashes and attribute bytes already live in the binary.
    // Validate the fixed ABI without rebuilding transient name/attribute
    // vectors or revalidating each image string as UTF-8 at agent startup.
    let mapped = keys
        .iter()
        .enumerate()
        .any(|(i, key)| key.slot as usize != i);
    if mapped {
        // A permutation keeps the next position free as the next key's slot.
        let mut taken = vec![false; keys.len()];
        for key in keys {
            let slot = key.slot as usize;
            if slot >= keys.len() || taken[slot] {
                return Err(0);
            }
            taken[slot] = true;
        }
    }
    for key in keys {
        if key.slot >= d.slot_count {
            return Err(0);
        }
        let admitted = match KEYS {
            KEYS_LAZY => lazy_key_ok(key),
            _ => bound_key_ok(key, d),
        };
        if !admitted {
            return Err(0);
        }
    }
    // Case 2c: a class family's instances have the class's prototype, and
    // its id is its descriptor record's, unless the descriptor names the
    // instances' prototype itself: `%Object.prototype%` (3, `exports`) or
    // null (1, `require.cache`). The shape's identity is the one record of
    // that link.
    // A lazy receiver's class tag names its native kind. Its prototype
    // identity is the descriptor's fact, not a class evaluation's link.
    let class_family = !lazy && d.class_id != 0;
    let proto_id = match d.proto.addr() {
        3 => super::super::shapes::PROTO_ID_DEFAULT,
        1 => super::super::shapes::PROTO_ID_NULL,
        _ if class_family => super::super::shapes::class_proto_id(d.class_id),
        0 => super::super::shapes::PROTO_ID_DEFAULT,
        2 if d.variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR => {
            match function_proto {
                Some(id) if id != 0 => id,
                _ => return Err(0),
            }
        }
        fam if fam >= LAYOUT_PROTO_TAG_END => proto_family_serial(d.proto),
        _ => return Err(0),
    };
    if local != 0 && bound.is_empty() {
        // Reserve the id the registration would take; the bind installs it.
        let proposed = if proto_id == super::super::shapes::PROTO_ID_DEFAULT
            || d.variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR
        {
            requested
        } else {
            super::super::shapes::allocate_declared_layout_id(proto_id).0
        };
        let _ = word.compare_exchange(0, proposed, Ordering::AcqRel, Ordering::Acquire);
        return Err((*desc).layout_id);
    }
    Ok(RowFacts {
        keys,
        kind,
        function_summary,
        function,
        mapped,
        existing,
        class_family,
        proto_id,
        requested,
        lazy,
    })
}

pub(super) unsafe fn register_bound(
    desc: *const PerryLayoutDesc,
    requested: u32,
    bound: &[u64],
    function_proto: Option<u64>,
) -> u32 {
    let facts = match row_facts::<KEYS_BOUND>(desc, requested, bound, function_proto) {
        Ok(facts) => facts,
        Err(id) => return id,
    };
    let d = &*desc;
    let (keys, function, mapped) = (facts.keys, facts.function, facts.mapped);
    let key_count = keys.len() as u32;
    // Compatibility key arrays are owned by the one authoritative shape
    // record, and are shared by the canonical key trie when names/attrs agree.
    let _no_gc = crate::gc::GcSuppressScope::new();
    // The canonical handle's view: the list and its trie node, which the
    // shape record carries (CKI S1).
    let list_keys = if key_count == 0 {
        super::super::ObjectKeys::NONE
    } else if super::super::image_keys::prepare(keys, d.image_keys) {
        super::super::canonical_keys::static_seed::link_image_list(
            &super::super::canonical_keys::SharedLayout::shape_cache_entry(),
            keys,
            d.image_keys,
        )
        .expect("descriptor image list agrees with its keys and attributes")
        .view()
    } else if let Some(canonical) = (!function)
        .then(|| {
            super::super::canonical_keys::from_static_layout(
                &super::super::canonical_keys::SharedLayout::shape_cache_entry(),
                keys,
                mapped,
            )
        })
        .flatten()
    {
        canonical.view()
    } else {
        let list = super::super::key_attrs::alloc_canonical_key_list(
            key_count,
            true,
            mapped || keys.iter().any(|key| key.attrs != 0),
        );
        let values = crate::array::array_elements_ptr(list);
        let attributes = super::super::key_attrs::keys_attrs(list);
        let mut next = 0;
        for (i, key) in keys.iter().enumerate() {
            // GC_STORE_AUDIT(INIT): unpublished list, under suppression.
            *values.add(i) = if function && (*key.atom).kind == crate::codegen_abi::ATOM_KIND_STRING
            {
                let atom = crate::string::property_atom((*key.atom).string);
                crate::value::JSValue::string_ptr(atom.cast_mut()).bits()
            } else {
                layout_key_word(key, bound, &mut next)
            };
            if !attributes.is_null() {
                super::super::key_attrs::attrs_write(attributes, i as u32, key.attrs, &*key.atom);
                if mapped {
                    super::super::key_attrs::attrs_write_slot(attributes, i as u32, key.slot);
                }
            }
        }
        (*list).length = key_count;
        super::super::canonical_keys::canonicalize_owned(
            &super::super::canonical_keys::SharedLayout::shape_cache_entry(),
            list,
            key_count,
        )
        .view()
    };
    install_row(desc, &facts, list_keys)
}

/// Install a validated row's shape on its canonical key list and publish its
/// identity. Shared by both registrars; allocates nothing on the GC heap.
#[inline(always)]
unsafe fn install_row(
    desc: *const PerryLayoutDesc,
    facts: &RowFacts,
    list_keys: super::super::ObjectKeys,
) -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    let word = AtomicU32::from_ptr(std::ptr::addr_of!((*desc).shape_id).cast_mut());
    let d = &*desc;
    #[cfg(any(test, debug_assertions))]
    if is_runtime_layout_word(d.layout_id) {
        RUNTIME_LAYOUT_INSTALLS.with(|n| n.set(n.get() + 1));
    }
    let RowFacts {
        keys,
        kind,
        function_summary,
        function,
        existing,
        class_family,
        proto_id,
        requested,
        lazy,
        ..
    } = *facts;
    let key_count = keys.len() as u32;
    let slot_count = d.slot_count;
    let rep = d.rep;
    let integrity = d.integrity as u16;
    let list = list_keys.arr();
    let summary = super::super::key_attrs::keys_summary(list, key_count) | function_summary;
    // A carried prototype layout is a marked prototype's: its own semantic
    // generation, shared with no other layout.
    let image = d.variant == PROTOTYPE_IMAGE_VARIANT;
    let generation = if image {
        super::super::shapes::fresh_semantic_generation()
    } else {
        0
    };
    if function {
        let word = super::super::shapes::publish_shape_result(
            super::super::shapes::shape_descriptor_intern_with_rep(
                list_keys,
                key_count,
                slot_count,
                generation,
                kind,
                0,
                proto_id,
                summary,
                rep,
                &[],
                None,
                integrity,
            ),
        );
        // A per-evaluation record is NOT an external carrier: the objects born
        // on it (and the prototype they name) keep it live, and it retires
        // with the last of them. The caller holds GC suppression from here to
        // the birth's stamp (function_family_birth::allocate_layout).
        return word.0;
    }
    let proposed = if let Some(id) = existing {
        id
    } else if is_runtime_layout_word(d.layout_id) {
        super::super::shapes::allocate_declared_layout_id(proto_id).0
    } else if lazy {
        super::super::shapes::allocate_declared_layout_for_kind(proto_id, kind).0
    } else if function {
        super::super::shapes::allocate_declared_layout_id(proto_id).0
    } else if proto_id == super::super::shapes::PROTO_ID_DEFAULT
        // A class family keeps its section-ordered identity: a class
        // identity is implied, which a plain id carries. A family linked to
        // null (`require.cache`) names the null identity, which only a
        // null-kind id carries: it takes an allocated one, beside the
        // null-kind word the codegen gave its descriptor.
        || (class_family && proto_id != super::super::shapes::PROTO_ID_NULL)
        || d.variant == crate::codegen_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR
    {
        requested
    } else {
        super::super::shapes::allocate_declared_layout_id(proto_id).0
    };
    // End the descriptor borrow before the startup-only id publication.
    let id = if let Some(id) = existing {
        id
    } else {
        match word.compare_exchange(0, proposed, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => proposed,
            Err(other) => {
                assert!(
                    super::super::shapes::is_shape_id(other),
                    "descriptor identity cell holds a ShapeId"
                );
                other
            }
        }
    };
    if super::super::shapes::shape_descriptor_by_id(ShapeId(id)).is_none() {
        super::super::shapes::install_declared_layout(
            ShapeId(id),
            LayoutWord((*desc).layout_id),
            if class_family || literal_birth::contains(desc) {
                descriptor_index(desc).map_or(0, |i| i + 1)
            } else {
                0
            },
            list_keys,
            key_count,
            slot_count,
            kind,
            proto_id,
            summary,
            rep,
            integrity,
            generation,
        );
    }
    super::super::shapes::note_external_shape_carrier(
        super::super::shapes::shape_descriptor_for_word(LayoutWord((*desc).layout_id)),
    );
    (*desc).layout_id
}

#[cfg(test)]
mod lazy_contract_tests {
    use super::*;

    #[test]
    fn lazy_row_facts_admit_only_image_key_kinds() {
        unsafe {
            let source = crate::object::runtime_layouts::ITERATOR_RESULT::desc();
            let base_atom = &*(*(*source).keys).atom;
            for kind in [
                crate::codegen_abi::ATOM_KIND_STRING,
                crate::codegen_abi::ATOM_KIND_WELL_KNOWN_SYMBOL,
                crate::codegen_abi::ATOM_KIND_REGISTERED_SYMBOL,
                crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL,
                crate::codegen_abi::ATOM_KIND_PROGRAM_SYMBOL,
            ] {
                let atom = StaticAtom { hash: base_atom.hash, string: base_atom.string, kind, index: 0, _pad: [0; 3] };
                let key = PerryLayoutKey { atom: &atom, slot: 0, attrs: 0, ..std::mem::zeroed() };
                let mut desc = std::ptr::read(source);
                desc.shape_id = 0;
                desc.key_count = 1;
                desc.slot_count = 1;
                desc.keys = &key;
                let facts = row_facts::<KEYS_LAZY>(&desc, 0, &[], None);
                assert_eq!(facts.is_ok(), kind == crate::codegen_abi::ATOM_KIND_STRING, "kind {kind}");
            }
        }
    }

    #[test]
    fn lazy_row_facts_use_the_declared_prototype_with_native_class_headers() {
        crate::test_support::bootstrap_test_thread();
        unsafe {
            let family = PerryProtoFamily { serial: 58, image_prototype: std::ptr::null() };
            let mut desc = std::ptr::read(crate::object::runtime_layouts::ITERATOR_RESULT::desc());
            desc.shape_id = 0;
            desc.key_count = 0;
            desc.slot_count = 0;
            desc.class_id = 77;
            desc.proto = &family;
            let image = row_facts::<KEYS_LAZY>(&desc, 0, &[], None).ok().unwrap();
            assert_eq!(image.proto_id, 58, "the image declares the prototype identity");
            assert!(!image.class_family);
            let birth = row_facts::<KEYS_BOUND>(&desc, 0, &[], None).ok().unwrap();
            assert!(birth.class_family, "ordinary class-family births retain their existing rule");
            assert_eq!(birth.proto_id, super::super::super::shapes::PROTO_ID_CLASS | 77);
        }
    }

    #[test]
    #[should_panic(expected = "GC allocation inside lazy publication")]
    fn lazy_publication_guard_rejects_gc_allocations() {
        crate::gc::ensure_gc_initialized();
        let _window = crate::object::layout_desc::NoGcAlloc::enter();
        let _ = crate::arena::arena_alloc_gc(16, 8, crate::gc::GC_TYPE_OBJECT);
    }
}
