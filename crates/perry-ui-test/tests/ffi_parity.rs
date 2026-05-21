use perry_ui_test::{Support, FEATURES};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Extract `perry_ui_*` / `perry_system_*` FFI symbols from Rust source.
/// Matches: `pub extern "C" fn perry_...(` across one or more lines.
fn extract_ffi_symbols(source: &str) -> HashSet<String> {
    let mut symbols = HashSet::new();
    for line in source.lines() {
        let trimmed = line.trim();
        // Match: pub extern "C" fn perry_...(
        if let Some(rest) = trimmed.strip_prefix("pub extern \"C\" fn ") {
            if let Some(paren) = rest.find('(') {
                let name = &rest[..paren];
                if name.starts_with("perry_ui_") || name.starts_with("perry_system_") {
                    symbols.insert(name.to_string());
                }
            }
        }
    }
    symbols
}

/// Extract native FFI symbols from a platform crate's whole `src` tree.
///
/// The native UI crates now split linker-visible `#[no_mangle]` FFI exports
/// across topical modules under `src/ffi*/` or `src/lib_ffi/`, so scanning
/// only `lib.rs` misses the actual exported surface.
fn extract_native_crate_symbols(src_dir: &Path) -> HashSet<String> {
    fn visit(path: &Path, symbols: &mut HashSet<String>) {
        if path.is_dir() {
            let Ok(entries) = fs::read_dir(path) else {
                return;
            };
            for entry in entries.flatten() {
                visit(&entry.path(), symbols);
            }
            return;
        }

        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            return;
        }

        let Ok(source) = fs::read_to_string(path) else {
            return;
        };
        symbols.extend(extract_ffi_symbols(&source));
    }

    let mut symbols = HashSet::new();
    visit(src_dir, &mut symbols);
    symbols
}

/// Extract `perry_ui_*` / `perry_system_*` symbols from web runtime JS.
/// Matches: `function perry_...(` or `function perry_...(`
fn extract_web_symbols(source: &str) -> HashSet<String> {
    let mut symbols = HashSet::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("function ") {
            if let Some(paren) = rest.find('(') {
                let name = &rest[..paren];
                if name.starts_with("perry_ui_") || name.starts_with("perry_system_") {
                    symbols.insert(name.to_string());
                }
            }
        }
    }
    symbols
}

/// Verify that all features marked Supported/Stub in the matrix exist in source.
/// Also warn about untracked symbols found in source but not in the matrix.
fn check_platform(
    platform_name: &str,
    symbols: &HashSet<String>,
    get_support: impl Fn(&perry_ui_test::Feature) -> Support,
    get_expected_name: impl Fn(&perry_ui_test::Feature) -> &str,
) {
    let mut missing = Vec::new();
    let mut expected_names: HashSet<String> = HashSet::new();

    for f in FEATURES {
        let support = get_support(f);
        let expected = get_expected_name(f);
        expected_names.insert(expected.to_string());

        match support {
            Support::Supported | Support::Stub => {
                if !symbols.contains(expected) {
                    missing.push(format!("  {} (expected as '{}')", f.name, expected));
                }
            }
            Support::Unsupported => {
                if symbols.contains(expected) {
                    eprintln!(
                        "WARN: {} has '{}' but matrix says Unsupported — consider updating the matrix",
                        platform_name, expected
                    );
                }
            }
        }
    }

    // Detect untracked symbols
    let untracked: Vec<_> = symbols
        .iter()
        .filter(|s| !expected_names.contains(s.as_str()))
        .collect();
    if !untracked.is_empty() {
        let mut sorted: Vec<_> = untracked.into_iter().collect();
        sorted.sort();
        eprintln!(
            "WARN: {} has {} untracked symbol(s) not in the feature matrix:",
            platform_name,
            sorted.len()
        );
        for s in &sorted {
            eprintln!("  {}", s);
        }
    }

    if !missing.is_empty() {
        panic!(
            "{} is missing {} expected symbol(s):\n{}",
            platform_name,
            missing.len(),
            missing.join("\n")
        );
    }
}

// ── Platform Tests ───────────────────────────────────────────────────────────

macro_rules! native_platform_test {
    ($test_name:ident, $platform_name:expr, $src_path:expr, $field:ident) => {
        #[test]
        fn $test_name() {
            let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
            let symbols = extract_native_crate_symbols(&manifest_dir.join($src_path));
            check_platform($platform_name, &symbols, |f| f.$field, |f| f.name);
        }
    };
}

native_platform_test!(test_macos, "macOS", "../perry-ui-macos/src", macos);
native_platform_test!(test_ios, "iOS", "../perry-ui-ios/src", ios);
native_platform_test!(test_android, "Android", "../perry-ui-android/src", android);
native_platform_test!(test_gtk4, "GTK4", "../perry-ui-gtk4/src", gtk4);
native_platform_test!(test_windows, "Windows", "../perry-ui-windows/src", windows);

#[test]
fn test_web() {
    let source = include_str!("../../perry-codegen-js/src/web_runtime.js");
    let symbols = extract_web_symbols(source);
    check_platform("Web", &symbols, |f| f.web, |f| f.web_name.unwrap_or(f.name));
}
