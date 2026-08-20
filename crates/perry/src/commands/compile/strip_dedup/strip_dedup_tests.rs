//! Unit tests for the strip/dedup pipeline — split out of `strip_dedup.rs`
//! to keep the parent under the 2000-line file-size gate (#8455 grew the
//! evidence handling). Child module of `strip_dedup`, so `super::` sees its
//! private items exactly as the inline `mod strip_dedup_tests` did.

use super::{force_localize_symbol, is_panic_unwind_symbol, parse_nm_archive_output};

#[test]
fn panic_unwind_classification_matches_dwref() {
    // The compiler-emitted `DW.ref.rust_eh_personality` substring-matches
    // `rust_eh_personality`; both must be treated as panic/unwind so the
    // well-known localizer skips them on ELF (PIE relocation breakage).
    assert!(is_panic_unwind_symbol("rust_eh_personality"));
    assert!(is_panic_unwind_symbol("DW.ref.rust_eh_personality"));
    assert!(is_panic_unwind_symbol("rust_begin_unwind"));
    assert!(is_panic_unwind_symbol("rust_panic"));
    // Allocator shims and ordinary symbols are not panic/unwind.
    assert!(!is_panic_unwind_symbol("__rust_alloc"));
    assert!(!is_panic_unwind_symbol("__rdl_dealloc"));
    assert!(!is_panic_unwind_symbol("js_fetch_with_options"));

    // Candidate collection is unchanged: allocator shims and the
    // panic/unwind group are both still force-localize candidates (the ELF
    // skip happens per-object in the well-known localizer, not here).
    assert!(force_localize_symbol("rust_eh_personality"));
    assert!(force_localize_symbol("__rust_alloc"));
    assert!(force_localize_symbol("js_stdlib_init_dispatch"));
    assert!(!force_localize_symbol("js_some_regular_export"));
}

#[test]
fn parser_handles_bare_member_headers() {
    let nm_out = "\
member_one.o:
_sym_a
_sym_b

member_two.o:
_sym_c
";
    let map = parse_nm_archive_output(nm_out);
    assert_eq!(map.len(), 2);
    assert!(map["member_one.o"].contains("_sym_a"));
    assert!(map["member_one.o"].contains("_sym_b"));
    assert_eq!(map["member_one.o"].len(), 2);
    assert_eq!(map["member_two.o"].len(), 1);
    assert!(map["member_two.o"].contains("_sym_c"));
}

#[test]
fn parser_strips_archive_wrapper_from_header() {
    // Some llvm-nm versions wrap each member as
    // `archive.a(member.o):` — we want the bare member name so the
    // map keys match `ar t` output.
    let nm_out = "\
/path/to/lib.a(perry_runtime-abc.cgu.0.rcgu.o):
_SYM
";
    let map = parse_nm_archive_output(nm_out);
    assert_eq!(map.len(), 1);
    assert!(map.contains_key("perry_runtime-abc.cgu.0.rcgu.o"));
}

#[test]
fn parser_handles_bsd_symbol_lines() {
    let nm_out = "\
member_one.o:
0000000000000000 T _sym_a
0000000000000010 r .Lprivate

member_two.o:
0000000000000000 T js_stdlib_init_dispatch
";
    let map = parse_nm_archive_output(nm_out);
    assert_eq!(map.len(), 2);
    assert!(map["member_one.o"].contains("_sym_a"));
    assert!(map["member_one.o"].contains(".Lprivate"));
    assert!(map["member_two.o"].contains("js_stdlib_init_dispatch"));
}

#[test]
fn parser_skips_empty_members() {
    let nm_out = "\
empty.o:

next.o:
_sym
";
    let map = parse_nm_archive_output(nm_out);
    // Empty.o produces no entry — `member_syms.is_empty()` is the
    // call-site guard that keeps zero-symbol members anyway.
    assert!(!map.contains_key("empty.o"));
    assert_eq!(map["next.o"].len(), 1);
}

#[test]
fn subset_check_prunes_only_full_overlap() {
    // The actual filter logic: keep a member iff at least one of its
    // symbols is unique (i.e. not in the provided set). This pins
    // down the v0.5.320 #181 invariant — a member with a unique
    // generic monomorphization (not in standalone runtime/stdlib)
    // must be KEPT even if its name happens to match the pattern.
    let nm_out = "\
fully_dup.o:
_a
_b

unique_mono.o:
_a
_specific_to_this_lib

empty_marker.o:
";
    let by_member = parse_nm_archive_output(nm_out);
    let provided: std::collections::HashSet<String> =
        ["_a".to_string(), "_b".to_string(), "_z".to_string()]
            .into_iter()
            .collect();

    // fully_dup.o → all symbols provided → drop
    let m1 = &by_member["fully_dup.o"];
    assert!(!m1.is_empty() && m1.iter().all(|s| provided.contains(s)));

    // unique_mono.o → has _specific_to_this_lib not in provided → keep
    let m2 = &by_member["unique_mono.o"];
    assert!(!m2.is_empty() && !m2.iter().all(|s| provided.contains(s)));

    // empty_marker.o → no entry; call site keeps it defensively.
    assert!(!by_member.contains_key("empty_marker.o"));
}

#[cfg(target_os = "windows")]
#[test]
fn coff_archive_dedup_drops_only_fully_provided_members() {
    use super::{
        collect_archive_symbols_flat, find_llvm_tool_or_beside_lld, rebuild_archive,
        strip_duplicate_objects_from_lib,
    };
    use std::path::Path;
    use std::process::Command;

    fn compile_object(source: &Path, output: &Path, crate_name: &str) {
        let result = Command::new("rustc")
            .arg("--crate-name")
            .arg(crate_name)
            .arg("--crate-type=lib")
            .arg("--emit=obj")
            .arg("-Cpanic=abort")
            .arg(source)
            .arg("-o")
            .arg(output)
            .output()
            .expect("rustc must run");
        assert!(
            result.status.success(),
            "rustc failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    let temp = tempfile::tempdir().expect("temporary COFF fixture directory");
    let duplicate_source = temp.path().join("runtime.rs");
    let unique_source = temp.path().join("ui.rs");
    std::fs::write(
        &duplicate_source,
        "#[no_mangle]\npub extern \"C\" fn runtime_canonical() {}\n",
    )
    .unwrap();
    std::fs::write(
        &unique_source,
        "#[no_mangle]\npub extern \"C\" fn ui_only_symbol() {}\n",
    )
    .unwrap();

    let duplicate_object = temp.path().join("runtime.obj");
    let unique_object = temp.path().join("ui.obj");
    compile_object(&duplicate_source, &duplicate_object, "runtime_fixture");
    compile_object(&unique_source, &unique_object, "ui_fixture");

    let llvm_ar =
        find_llvm_tool_or_beside_lld("llvm-ar").expect("Windows LLVM must provide llvm-ar");
    let llvm_nm =
        find_llvm_tool_or_beside_lld("llvm-nm").expect("Windows LLVM must provide llvm-nm");
    let runtime = temp.path().join("perry_runtime.lib");
    let ui = temp.path().join("perry_ui_windows.lib");
    let ui_rlib = temp.path().join("libperry_ui_windows.rlib");
    // Current rustc uses opaque CGU components such as
    // `.1v150fxu9jccif28r12uax7fb.02s2fqd.rcgu.o`, without the literal
    // `.cgu.` / `-cgu.` segments recognized by the old extraction code.
    let current_rustc_cgu = temp
        .path()
        .join("perry_ui_windows-deadbeef.opaque.codegen.rcgu.o");
    std::fs::copy(&unique_object, &current_rustc_cgu).unwrap();
    rebuild_archive(
        &llvm_ar,
        &runtime,
        std::slice::from_ref(&duplicate_object),
        true,
    )
    .unwrap();
    rebuild_archive(
        &llvm_ar,
        &ui,
        &[duplicate_object.clone(), unique_object],
        true,
    )
    .unwrap();
    rebuild_archive(
        &llvm_ar,
        &ui_rlib,
        std::slice::from_ref(&current_rustc_cgu),
        true,
    )
    .unwrap();

    let trimmed = strip_duplicate_objects_from_lib(&ui).expect("COFF dedup must succeed");
    let symbols = collect_archive_symbols_flat(&llvm_nm, &trimmed);
    assert!(symbols.contains("ui_only_symbol"));
    assert!(!symbols.contains("runtime_canonical"));
}

/// #8455: the dedup evidence set must equal the archives actually on the
/// link line. A member whose symbols are provided ONLY by perry-stdlib
/// must be dropped when stdlib is linked and KEPT when it is not — before
/// the fix, a pure-UI macOS link (runtime only) dropped the UI staticlib's
/// bundled-std members on stdlib evidence and failed with
/// `Undefined symbols: <Stdout as Write>::flush`.
#[test]
fn stdlib_evidence_gate_keeps_members_when_stdlib_is_not_linked() {
    use super::{
        collect_archive_symbols_flat, find_llvm_tool_or_beside_lld, rebuild_archive,
        strip_duplicate_objects_from_lib_with_evidence, StdlibEvidence,
    };
    use std::path::Path;
    use std::process::Command;

    fn compile_object(source: &Path, output: &Path, crate_name: &str) {
        let result = Command::new("rustc")
            .arg("--crate-name")
            .arg(crate_name)
            .arg("--crate-type=lib")
            .arg("--emit=obj")
            .arg("-Cpanic=abort")
            .arg(source)
            .arg("-o")
            .arg(output)
            .output()
            .expect("rustc must run");
        assert!(
            result.status.success(),
            "rustc failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    let temp = tempfile::tempdir().expect("temporary evidence fixture directory");
    let stdlib_only_source = temp.path().join("stdlib_dup.rs");
    let unique_source = temp.path().join("ui.rs");
    std::fs::write(
        &stdlib_only_source,
        "#[no_mangle]\npub extern \"C\" fn stdlib_provided_symbol() {}\n",
    )
    .unwrap();
    std::fs::write(
        &unique_source,
        "#[no_mangle]\npub extern \"C\" fn ui_unique_symbol() {}\n",
    )
    .unwrap();

    let runtime_source = temp.path().join("runtime.rs");
    std::fs::write(
        &runtime_source,
        "#[no_mangle]\npub extern \"C\" fn runtime_canonical_symbol() {}\n",
    )
    .unwrap();

    let stdlib_only_object = temp.path().join("stdlib_dup.obj");
    let unique_object = temp.path().join("ui.obj");
    let runtime_object = temp.path().join("runtime.obj");
    compile_object(&stdlib_only_source, &stdlib_only_object, "stdlib_fixture");
    compile_object(&unique_source, &unique_object, "ui_fixture");
    compile_object(&runtime_source, &runtime_object, "runtime_fixture");

    let llvm_ar = find_llvm_tool_or_beside_lld("llvm-ar").expect("llvm-ar present");
    let llvm_nm = find_llvm_tool_or_beside_lld("llvm-nm").expect("llvm-nm present");
    // The stdlib archive lives NEXT TO the UI lib, exactly where the
    // legacy self-location would find it — proving NotLinked overrides
    // discoverability, not just absence.
    let stdlib = temp.path().join("libperry_stdlib.a");
    let runtime = temp.path().join("libperry_runtime.a");
    let ui = temp.path().join("libperry_ui_fixture.a");
    rebuild_archive(
        &llvm_ar,
        &stdlib,
        std::slice::from_ref(&stdlib_only_object),
        false,
    )
    .unwrap();
    rebuild_archive(
        &llvm_ar,
        &runtime,
        std::slice::from_ref(&runtime_object),
        false,
    )
    .unwrap();
    rebuild_archive(
        &llvm_ar,
        &ui,
        &[stdlib_only_object.clone(), unique_object],
        false,
    )
    .unwrap();

    // stdlib NOT on the link line: the stdlib-covered member must survive.
    let kept = strip_duplicate_objects_from_lib_with_evidence(&ui, StdlibEvidence::NotLinked)
        .expect("dedup with NotLinked evidence must succeed");
    // Mach-O prepends `_` to C-ABI names; COFF/ELF do not.
    fn has(symbols: &std::collections::HashSet<String>, name: &str) -> bool {
        symbols.contains(name) || symbols.contains(&format!("_{name}"))
    }
    let kept_symbols = collect_archive_symbols_flat(&llvm_nm, &kept);
    assert!(
        has(&kept_symbols, "stdlib_provided_symbol"),
        "a member is only droppable against archives that are actually \
         linked; with stdlib off the link line its copy must survive: {kept_symbols:?}"
    );
    assert!(has(&kept_symbols, "ui_unique_symbol"));

    // stdlib IS on the link line: the same member is a pure duplicate.
    let trimmed =
        strip_duplicate_objects_from_lib_with_evidence(&ui, StdlibEvidence::Linked(&stdlib))
            .expect("dedup with Linked evidence must succeed");
    let trimmed_symbols = collect_archive_symbols_flat(&llvm_nm, &trimmed);
    assert!(!has(&trimmed_symbols, "stdlib_provided_symbol"));
    assert!(has(&trimmed_symbols, "ui_unique_symbol"));
}

#[test]
fn rust_codegen_unit_recognizes_current_and_legacy_member_names() {
    assert!(super::is_rust_codegen_unit(
        "perry_ui_windows-deadbeef.opaque.codegen.rcgu.o"
    ));
    assert!(super::is_rust_codegen_unit(
        "perry_ui_windows-deadbeef.perry_ui_windows.hash-cgu.0.rcgu.o"
    ));
    assert!(!super::is_rust_codegen_unit("allocator_shim.o"));
    assert!(!super::is_rust_codegen_unit("lib.rmeta"));
}

#[test]
fn extracted_path_qualified_archive_member_falls_back_to_basename() {
    let temp = tempfile::tempdir().unwrap();
    let extracted = temp.path().join("loader_impl.obj");
    std::fs::write(&extracted, b"native object fixture").unwrap();

    assert_eq!(
        super::extracted_archive_member(
            temp.path(),
            "obj/edge_embedded_browser/client/win/WebView2LoaderLib/loader_impl.obj",
        ),
        Some(extracted)
    );
}

#[test]
fn windows_import_members_include_dll_and_driver_archives() {
    assert!(super::is_windows_import_archive_member("uxtheme.dll"));
    assert!(super::is_windows_import_archive_member("winspool.drv"));
    assert!(super::is_windows_import_archive_member("WINSPool.DRV"));
    assert!(!super::is_windows_import_archive_member("loader_impl.obj"));
}
