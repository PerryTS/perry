use super::*;

#[test]
fn comdat_atoms_preserve_identity_and_keep_local_bindings_distinct() {
    for target in [
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "aarch64-apple-darwin",
    ] {
        COLOURS.with(|c| c.borrow_mut().clear());
        let mut m = LlModule::new(target);
        let mut strings = StringPool::with_prefix("comdat".into());
        let mut emitted = BTreeMap::new();
        let string = atom(&mut m, &mut strings, &mut emitted, "shared").unwrap();
        let mut weak = vec![string];
        for key in [
            create_families::ImageKey::WellKnown(0),
            create_families::ImageKey::Registered("shared".into()),
        ] {
            weak.push(symbol_atom(&mut m, &mut strings, &mut emitted, "a", &key).0);
        }
        let key = create_families::ImageKey::Local(1);
        let a = symbol_atom(&mut m, &mut strings, &mut emitted, "a", &key).0;
        let b = symbol_atom(&mut m, &mut strings, &mut emitted, "b", &key).0;
        assert_ne!(a, b);
        // Two codegen units must preserve weak ownership and declare COMDATs
        // only alongside their sole defining unit, including orphaned atoms.
        for name in ["first", "second"] {
            let block = m
                .define_function(name, crate::types::PTR, vec![])
                .create_block("entry");
            block.ret(crate::types::PTR, &format!("@{}", weak[0]));
        }
        let whole = m.to_ir();
        #[cfg(feature = "llvm-inprocess")]
        assert_eq!(
            m.skeleton_ir().contains(" = comdat any"),
            !target.contains("apple")
        );
        let units = m.render_codegen_units(2);
        for name in &weak {
            let storage = if target.contains("windows") {
                "constant"
            } else {
                "global"
            };
            let definition = format!("@{name} = weak_odr {storage}");
            assert!(whole.contains(&definition));
            assert!(units.iter().any(|ir| ir.contains(&definition)));
            for ir in std::iter::once(&whole).chain(units.iter()) {
                if ir.contains(&definition) {
                    assert_eq!(
                        ir.contains(&format!("${name} = comdat any")),
                        !target.contains("apple")
                    );
                    if target.contains("windows") {
                        assert!(ir.contains(&format!(" /INCLUDE:{name}")));
                    }
                } else {
                    assert!(!ir.contains(&format!("${name} = comdat any")));
                }
            }
        }
        for name in [a, b] {
            let storage = if target.contains("windows") {
                "constant"
            } else {
                "global"
            };
            assert!(whole.contains(&format!("@{name} = internal {storage}")));
            assert!(!whole.contains(&format!("${name} = comdat")));
            assert!(!whole.contains(&format!(" /INCLUDE:{name}")));
        }
    }
}

#[cfg(feature = "llvm-inprocess")]
#[test]
fn coff_comdat_atom_retention_survives_object_emission() {
    let target = "x86_64-pc-windows-msvc";
    let mut m = LlModule::new(target);
    m.add_coalesced_global(
        "iteration_atom",
        "@iteration_atom = weak_odr constant i64 123, section \".perryatm$m\", align 8".into(),
    );
    // Literal contents must never be mistaken for the global's attributes.
    m.add_coalesced_global(
        "literal",
        "@literal = weak_odr constant [17 x i8] c\"weak_odr global \\00\", section \".perrystr$m\", align 8".into(),
    );
    let ir = m.to_ir();
    assert!(!ir.contains(" /INCLUDE:literal"));
    let objects = crate::inprocess::compile_ll_to_object_inprocess(
        ir,
        target,
        &["-O2".into()],
        "comdat-atom-test",
        false,
    )
    .expect("COFF data COMDAT and retention directive must assemble");
    assert_eq!(objects.len(), 1);
    assert!(objects[0]
        .windows(b" /INCLUDE:iteration_atom".len())
        .any(|bytes| bytes == b" /INCLUDE:iteration_atom"));
}
