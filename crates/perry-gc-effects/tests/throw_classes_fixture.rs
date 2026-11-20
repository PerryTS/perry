//! Compiled archive witnesses. The source-owned carrier is copied into a
//! scratch workspace, so removing its declaration tests the actual build check.
#![cfg(feature = "analyzer")]
use perry_gc_effects::*;
use std::path::Path;

const DECL: &str = "crate::fn_info!(#[realm(throws = passes(0))] declare #[no_mangle] pub extern \"C\" fn fixture_passes(value: f64) -> f64 { value });";
const RULES: &str = "js exact fixture_js -- only the indirect edge enters program code\njs exact fixture_js_validate -- the validation arm is native, only its indirect edge enters program code\nprogram exact fixture_program -- the whole body is interpreted program execution\nthrow prefix perry_runtime::exception::throw_leaf:: -- each seeded cold leaf uses the shared exception transport\n";
const FIXTURES: [(&str, &[u8], &[u8]); 3] = [
    (
        "elf-x86_64",
        include_bytes!("fixtures/libthrow_elf-x86_64.a"),
        include_bytes!("fixtures/libthrow_elf-x86_64-raw.a"),
    ),
    (
        "macho-arm64",
        include_bytes!("fixtures/libthrow_macho-arm64.a"),
        include_bytes!("fixtures/libthrow_macho-arm64-raw.a"),
    ),
    (
        "coff-x86_64",
        include_bytes!("fixtures/libthrow_coff-x86_64.a"),
        include_bytes!("fixtures/libthrow_coff-x86_64-raw.a"),
    ),
];
fn prepare(root: &Path, bytes: &[u8]) -> std::path::PathBuf {
    let src = root.join("crates/perry-runtime/src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), DECL).unwrap();
    let archive = root.join("runtime.a");
    std::fs::write(&archive, bytes).unwrap();
    archive
}
fn generate(root: &Path, archive: &Path) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_perry-gc-effects"))
        .arg("--root")
        .arg(root)
        .arg("--rules")
        .arg(root.join("seeds.txt"))
        .arg("generate")
        .args(["--min-symbols", "1", "--target", "fixture", "--out"])
        .arg(root.join("table.tsv"))
        .arg(archive)
        .output()
        .unwrap()
}

#[test]
fn seeded_runtime_functions_have_all_four_classes_in_generated_tables() {
    for (format, bytes, _) in FIXTURES {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let archive = prepare(root, bytes);
        std::fs::write(root.join("seeds.txt"), RULES).unwrap();
        let result = generate(root, &archive);
        assert!(
            result.status.success(),
            "{format}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let table = read_table(&root.join("table.tsv")).unwrap();
        for (symbol, expected) in [
            ("fixture_none", ThrowClass::None),
            ("fixture_js", ThrowClass::None),
            ("fixture_js_validate", ThrowClass::Core),
            ("fixture_program", ThrowClass::None),
            ("fixture_core", ThrowClass::Core),
            ("fixture_passes", ThrowClass::Passes),
            ("fixture_open", ThrowClass::Open),
        ] {
            assert_eq!(table[symbol].throw_class, expected, "{format} {symbol}");
        }
        assert_eq!(table["fixture_passes"].passes_text(), "self:0");
    }
}

#[test]
fn scratch_removed_passes_declaration_fails_with_the_entry_name() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let archive = prepare(root, FIXTURES[0].1);
    std::fs::write(root.join("seeds.txt"), RULES).unwrap();
    let good = generate(root, &archive);
    assert!(
        good.status.success(),
        "{}",
        String::from_utf8_lossy(&good.stderr)
    );
    std::fs::write(
        root.join("crates/perry-runtime/src/lib.rs"),
        "#[no_mangle] pub extern \"C\" fn fixture_passes(value:f64)->f64 { value }",
    )
    .unwrap();
    std::fs::remove_file(root.join("table.tsv")).unwrap();
    let bad = generate(root, &archive);
    let error = String::from_utf8_lossy(&bad.stderr);
    assert!(!bad.status.success(), "removed declaration passed");
    assert!(error.contains("fixture_passes"), "{error}");
    assert!(
        !root.join("table.tsv").exists(),
        "failed build wrote a table"
    );
}

#[test]
fn scratch_runtime_raw_entry_call_fails_the_graph_check() {
    for (format, _, planted) in FIXTURES {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let archive = prepare(root, planted);
        std::fs::write(root.join("seeds.txt"), RULES).unwrap();
        let bad = generate(root, &archive);
        let error = String::from_utf8_lossy(&bad.stderr);
        assert!(!bad.status.success(), "{format}: raw call passed");
        assert!(
            error.contains("fixture_raw") && error.contains("raw js_throw"),
            "{format}: {error}"
        );
    }
}
