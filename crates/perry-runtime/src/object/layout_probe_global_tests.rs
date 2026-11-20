//! DEC 243(5): globalThis is an ordinary object whose builtin names are
//! atoms, so the probe word of its record proves absence: a miss on the
//! global object continues the walk instead of declining to the by-name
//! ladder.
use super::*;

#[test]
fn global_object_keys_are_atoms_and_a_miss_proves_absence() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| unsafe {
            crate::gc::initialize_runtime_fixture();
            let _no_move = crate::gc::GcSuppressScope::new();
            let global = crate::JSValue::from_bits(crate::object::js_get_global_this().to_bits());
            assert!(global.is_pointer());
            let global = global.as_pointer::<ObjectHeader>();
            let record = record_of(test_dir(), perry_abi::LayoutWord((*global).parent_class_id));
            assert!(!record.is_null() && (*record).present());
            let count = (*record).logical_key_count as usize;
            assert!(
                count > 100,
                "the global object lists its builtin names: {count}"
            );
            let keys = (*record).keys as usize as *const crate::array::ArrayHeader;
            let words = crate::array::array_elements_ptr(keys) as *const u64;
            let mut not_atoms = Vec::new();
            let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            for i in 0..count {
                let bits = *words.add(i);
                if bits == TAG_HOLE {
                    continue;
                }
                // A builtin name is the runtime's static image atom, in the
                // string image section. The missing-atom fallback of
                // `js_string_pool_atom` also makes image-MARKED strings, but
                // allocates them, so the marker alone would pass a name
                // `global_atoms.rs` does not declare.
                if key_word_hash(bits).is_some()
                    && (bits & TAG_MASK != STRING_TAG
                        || crate::value::addr_class::in_string_image_section_for_tests(
                            (bits & POINTER_MASK) as usize,
                        ))
                {
                    continue;
                }
                let text =
                    crate::string::js_string_key_bytes(crate::JSValue::from_bits(bits), &mut sso)
                        .map(|t| String::from_utf8_lossy(t).into_owned())
                        .unwrap_or_else(|| format!("{bits:#x}"));
                not_atoms.push(text);
            }
            assert!(
                not_atoms.is_empty(),
                "global keys that are not image atoms: {not_atoms:?}"
            );
            let word = derive_word(&*record);
            assert_eq!(
                word & (PROBE_READY | PROBE_DECLINE | PROBE_ATOMS),
                PROBE_READY | PROBE_ATOMS,
                "a miss on the global object must prove absence (word {word:#x})"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

/// NXPA2a8 (i): the global object is born on its static layout
/// (`runtime_layouts::GLOBAL_OBJECT`) and population only fills the declared
/// slots: after it the object still carries that layout's key list, so the
/// declared keys cost one publish (the birth), not one transition and mint each.
/// Red-first: with the birth reverted to `js_object_alloc(0, 0)` the global
/// walks one transition per key onto a heap key list; with the builtin
/// accessor install treating the born empty pair as a replacement, each of
/// the four accessors publishes a shape.
#[test]
fn global_object_is_born_on_its_static_layout_and_population_keeps_it() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| unsafe {
            crate::gc::initialize_runtime_fixture();
            let _no_move = crate::gc::GcSuppressScope::new();
            let global = crate::JSValue::from_bits(crate::object::js_get_global_this().to_bits());
            assert!(global.is_pointer());
            let global = global.as_pointer::<ObjectHeader>();
            let desc = crate::object::runtime_layouts::GLOBAL_OBJECT::desc();
            let id = crate::object::layout_desc::layout_id_of_word((*global).parent_class_id);
            let record = record_of(test_dir(), perry_abi::LayoutWord((*global).parent_class_id));
            let mut entries = Vec::new();
            if !record.is_null() {
                let keys = (*record).keys as usize as *const crate::array::ArrayHeader;
                for i in 0..(*record).logical_key_count {
                    let attrs = crate::object::key_attrs::keys_entry(keys, i);
                    let word = crate::object::object_keys(global).get(i);
                    let name = if word.is_string() {
                        let s = word.as_string_ptr();
                        String::from_utf8_lossy(std::slice::from_raw_parts(
                            crate::string::string_data(s),
                            (*s).byte_len as usize,
                        ))
                        .into_owned()
                    } else {
                        format!("{:#x}", word.bits())
                    };
                    entries.push(format!("{i}:{name}={attrs}"));
                }
            }
            // Population fills the born layout's slots: the global keeps the
            // layout's image key list, and the shapes on that list are the
            // birth layout and the realm seal's prototype mark (one semantic
            // transition): no key define, attribute or accessor install
            // published one.
            let keys = crate::object::object_keys(global);
            let family =
                crate::object::canonical_keys::with_list_shapes(keys.node(), |l| l.family.len());
            eprintln!(
                "global layout {id:#x} birth {:#x} family {family:?}",
                (*desc).layout_id
            );
            assert_eq!(
                entries.len(),
                (*desc).key_count as usize,
                "entries {entries:?}"
            );
            // A program's descriptor carries the image key list; the test
            // definition (DESIGN §3.2 item 10) has none, and its births take
            // the heap-list path.
            if !(*desc).image_keys.is_null() {
                assert_eq!(
                    keys.arr(),
                    (*desc).image_keys,
                    "the global left its image key list"
                );
            }
            assert_eq!(
                family,
                Some(2),
                "shapes published on the global's keys; entries {entries:?}"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

/// NXPA2a9 (ii): the builtin global names are image data, not a startup
/// seed: no record of theirs is in the static atom section the seed walks
/// (`image_atoms`), and the agent's atom for each is still its image string.
/// Red-first: with the records back in `perry_atoms` the seed walks every declared name.
#[test]
fn builtin_global_atoms_are_image_data_and_never_seeded() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| unsafe {
            crate::gc::initialize_runtime_fixture();
            let desc = crate::object::runtime_layouts::GLOBAL_OBJECT::desc();
            let keys = std::slice::from_raw_parts((*desc).keys, (*desc).key_count as usize);
            let mut checked = 0usize;
            let mut runtime_own = 0usize;
            for k in keys {
                let atom = k.atom;
                let string = (*atom).string;
                let bytes = std::slice::from_raw_parts(
                    crate::string::string_data(string),
                    (*string).byte_len as usize,
                );
                if bytes == b"setTimeout" || bytes.starts_with(b"#<perry:private-value:") {
                    // Runtime atoms of their own (timers, the intrinsic
                    // private slot), seeded with the runtime's other atoms.
                    runtime_own += 1;
                    continue;
                }
                let seeded = crate::object::layout_desc::image_atoms()
                    .any(|entry| std::ptr::eq(entry as *const _, atom as *const _));
                assert!(
                    !seeded,
                    "global atom {} is in the static seed",
                    String::from_utf8_lossy(bytes)
                );
                assert_eq!(
                    crate::string::atom_lookup(bytes, (*atom).hash),
                    Some(string),
                    "the atom of {}",
                    String::from_utf8_lossy(bytes)
                );
                checked += 1;
            }
            // Every declared key is either checked here or one of the two
            // runtime atoms; the layout's key count is the authority, so a
            // builtin added to the global layout needs no count edited here.
            assert_eq!(runtime_own, 2, "runtime-owned atoms among the global keys");
            assert_eq!(checked + runtime_own, (*desc).key_count as usize);
        })
        .unwrap()
        .join()
        .unwrap();
}
