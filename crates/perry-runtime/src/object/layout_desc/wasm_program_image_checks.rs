//! Linked wasm checks: bounds, weak atom identity and the agent's intern table.
use super::*;

pub(super) fn check_atoms() {
    let mut checked = 0;
    // The fixture's temporary heap input exists only to probe the real
    // intern entry point; no collection may run with that raw input live.
    let _no_gc = crate::gc::GcSuppressScope::new();
    unsafe {
        for atom in records::<StaticAtom>(atom_bounds()) {
            let Some(bytes) = atom_bytes(atom) else {
                continue;
            };
            if bytes.is_empty() {
                continue;
            }
            assert!(crate::string::is_image_string((*atom).string));
            // Longer keys are imageable, but are outside the runtime intern
            // table's existing eligibility contract.
            if bytes.len() > crate::string::INTERN_MAX_BYTE_LEN as usize {
                continue;
            }
            let heap = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            let interned = crate::string::js_string_intern(heap, (*atom).hash);
            assert_eq!(
                interned,
                (*atom).string,
                "program image atom must be the intern table's answer"
            );
            let copies = records::<StaticAtom>(atom_bounds())
                .filter(|other| atom_bytes(*other).is_some_and(|other_bytes| other_bytes == bytes))
                .count();
            assert_eq!(
                copies, 1,
                "weak_odr atom must have one program address: {:?}",
                bytes
            );
            if bytes == b"wrt2cSharedKey" {
                eprintln!("wasm atom coalescing: wrt2cSharedKey atom={:p} string={:p} intern={:p} copies={copies}",
                    atom, (*atom).string, interned);
            }
            checked += 1;
        }
    }
    eprintln!("wasm program image atoms: {checked} intern lookups passed");
    super::literal_birth::check_image_indices();
}
