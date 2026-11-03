//! Startup-registration objects on ELF: the registration pointer must land in
//! `.init_array`. Kept out of `stubs.rs` so the tests survive reverting it.

use crate::stubs::{generate_feature_installer_object, generate_static_shape_seed_object};

/// `(name, sh_type, sh_flags, sh_size)` of every section of a 64-bit
/// little-endian ELF object.
fn elf64_sections(obj: &[u8]) -> Vec<(String, u32, u64, u64)> {
    assert_eq!(&obj[..4], b"\x7fELF", "not an ELF object");
    assert_eq!((obj[4], obj[5]), (2, 1), "expected ELF64 little-endian");
    let u16at = |o: usize| u16::from_le_bytes(obj[o..o + 2].try_into().unwrap()) as usize;
    let u32at = |o: usize| u32::from_le_bytes(obj[o..o + 4].try_into().unwrap());
    let u64at = |o: usize| u64::from_le_bytes(obj[o..o + 8].try_into().unwrap());
    let (shoff, shentsize) = (u64at(0x28) as usize, u16at(0x3A));
    let (shnum, shstrndx) = (u16at(0x3C), u16at(0x3E));
    let header = |i: usize| shoff + i * shentsize;
    let strtab = u64at(header(shstrndx) + 0x18) as usize;
    (0..shnum)
        .map(|i| {
            let h = header(i);
            let name_at = strtab + u32at(h) as usize;
            let end = obj[name_at..].iter().position(|&b| b == 0).unwrap();
            let name = String::from_utf8(obj[name_at..name_at + end].to_vec()).unwrap();
            (name, u32at(h + 4), u64at(h + 8), u64at(h + 0x20))
        })
        .collect()
}

/// Root cause of the dead registration on Linux: `@llvm.global_ctors`
/// lowered to `.ctors` (the in-process TargetMachine has
/// `UseInitArray=false`), which glibc on aarch64 never runs, so no feature
/// installer and no shape seed was ever registered. The object must carry
/// the registration in a writable SHT_INIT_ARRAY section and no `.ctors`.
/// The seed unit registers nothing since WRT step 2: it is data (the seed
/// table, atoms, image strings and key list cells in their sections).
#[test]
fn elf_startup_registration_lands_in_init_array() {
    let shape = crate::BirthShape {
        keys: b"u\0".to_vec(),
        key_count: 1,
        live: 1,
        proto: crate::BirthProto::Literal,
        typed: None,
        rep: 0,
        constfn: Vec::new(),
        private: Vec::new(),
        brands: Vec::new(),
        attrs: Vec::new(),
    };
    for target in ["linux-aarch64", "linux"] {
        let installers = generate_feature_installer_object(
            &["js_runtime_install_dyn_eval".to_string()],
            Some(&["js_stdlib_install_fastify".to_string()]),
            &[],
            Some(target),
        )
        .expect("the installer unit compiles");
        let seed = generate_static_shape_seed_object(&[(0x1000_0042, shape.clone())], Some(target))
            .expect("the seed unit compiles");
        for (what, obj) in [("installers", installers)] {
            let sections = elf64_sections(&obj);
            let init = sections
                .iter()
                .find(|s| s.0 == ".init_array")
                .unwrap_or_else(|| panic!("{target} {what}: no .init_array in {sections:?}"));
            assert_eq!(
                init.1, 14,
                "{target} {what}: .init_array is not SHT_INIT_ARRAY"
            );
            assert_eq!(init.2 & 0x3, 0x3, "{target} {what}: .init_array must be WA");
            assert_eq!(
                init.3, 8,
                "{target} {what}: exactly one registration pointer"
            );
            assert!(
                sections.iter().any(|s| s.0 == ".rela.init_array"),
                "{target} {what}: the pointer must be relocated to the register function"
            );
            assert!(
                !sections.iter().any(|s| s.0 == ".ctors"),
                "{target} {what}: legacy .ctors is never run"
            );
        }
        let sections = elf64_sections(&seed);
        for section in ["perry_realm", "perry_atoms", "perry_strings"] {
            assert!(
                sections.iter().any(|s| s.0 == section),
                "{target} seed: no {section} in {sections:?}"
            );
        }
        assert!(
            !sections
                .iter()
                .any(|s| s.0 == ".init_array" || s.0 == ".ctors"),
            "{target} seed: the seed unit is data and registers nothing"
        );
    }
}
