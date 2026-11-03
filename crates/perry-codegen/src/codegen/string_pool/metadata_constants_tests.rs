//! A function's display name must stay addressable from every codegen unit
//! that emits its `$info` record.

use super::function_names;
use crate::module::LlModule;
use crate::strings::StringPool;
use crate::types::PTR;

const TARGETS: [&str; 4] = [
    "x86_64-apple-macosx13.0.0",
    "arm64-apple-macosx15.0.0",
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
];

/// `named` carries a display name; `first` and `second` each name its info,
/// so a two-unit split puts the record in both units.
fn fixture(target: &str) -> LlModule {
    let mut module = LlModule::new(target);
    module
        .define_function("named", PTR, vec![])
        .create_block("entry")
        .ret(PTR, "null");
    let names = function_names(
        &mut module,
        &StringPool::new(),
        "m_ts",
        true,
        &[("named".to_string(), "onAsync".to_string())],
    );
    assert_eq!(names.len(), 1);
    for name in ["first", "second"] {
        let block = module
            .define_function(name, PTR, vec![])
            .create_block("entry");
        let info = block.fn_info_ref("named");
        block.ret(PTR, &info);
    }
    module.emit_fn_infos(true);
    module.bind_generated_globals_to_image();
    module
}

/// The record's name field is `name - info`. A name declared `external` in
/// a unit that defines the record is an undefined minuend, which x86_64
/// Mach-O cannot relocate ("can not be undefined in a subtraction
/// expression"): every split module refused on Intel macOS.
#[test]
fn split_units_name_function_names_through_a_defined_image() {
    for target in TARGETS {
        let units = fixture(target).render_codegen_units(3);
        assert!(units.len() > 1, "{target}: the fixture must split");
        let mut records = 0;
        for unit in &units {
            assert!(!unit.contains(".fnname."), "{target}: {unit}");
            let Some(record) = unit
                .lines()
                .find(|line| line.starts_with("@named$info = ") && !line.contains(" external "))
            else {
                continue;
            };
            records += 1;
            assert!(
                record.contains("i64 sub (i64 ptrtoint (ptr getelementptr (i8, ptr getelementptr (i8, ptr @perry_image_literal_"),
                "{target}: {record}"
            );
            let image = record
                .split("ptr @perry_image_literal_")
                .nth(1)
                .and_then(|rest| rest.split(',').next())
                .expect("image symbol");
            let definition = format!("@perry_image_literal_{image} = ");
            let line = unit
                .lines()
                .find(|line| line.starts_with(&definition))
                .unwrap_or_else(|| panic!("{target}: no {definition} in {unit}"));
            assert!(
                !line.contains(" external "),
                "{target}: the record's minuend must be defined in its unit: {line}"
            );
        }
        assert!(records > 0, "{target}: no unit emitted the record");
    }
}

/// The same split, compiled unit by unit and partially linked as the
/// compiler does, for each Mach-O architecture. Before the fix the x86_64
/// unit that declared the alias failed to assemble.
#[cfg(all(feature = "llvm-inprocess", target_os = "macos"))]
#[test]
fn split_mach_o_units_with_function_names_assemble() {
    for target in &TARGETS[..2] {
        let _native = crate::codegen::helpers::NativeRootsPin::native();
        let units = fixture(target).render_codegen_units(3);
        let object = crate::linker::compile_units_to_object(&units, Some(target))
            .unwrap_or_else(|error| panic!("{target}: {error:#}"));
        assert!(!object.is_empty(), "{target}");
    }
}
