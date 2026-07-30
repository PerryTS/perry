use perry_ui_test::{Support, FEATURES};
use std::collections::{BTreeMap, HashMap, HashSet};
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct AbiSignature {
    args: Vec<AbiType>,
    ret: AbiType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AbiType {
    I64,
    F64,
    Void,
    Other(String),
}

impl std::fmt::Display for AbiType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::I64 => f.write_str("i64"),
            Self::F64 => f.write_str("f64"),
            Self::Void => f.write_str("void"),
            Self::Other(ty) => f.write_str(ty),
        }
    }
}

impl std::fmt::Display for AbiSignature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let args = self
            .args
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        write!(f, "({args}) -> {}", self.ret)
    }
}

fn parse_abi_type(source: &str) -> AbiType {
    let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
    match compact.as_str() {
        "" | "()" => AbiType::Void,
        "i64" => AbiType::I64,
        "f64" => AbiType::F64,
        _ => AbiType::Other(compact),
    }
}

fn split_top_level(source: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for (offset, ch) in source.char_indices() {
        match ch {
            '(' | '[' | '<' => depth += 1,
            ')' | ']' | '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&source[start..offset]);
                start = offset + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&source[start..]);
    parts
}

/// Parse linker-visible Rust FFI definitions from one source file.
///
/// This deliberately reads the declared Rust types rather than a built
/// library's symbol table: object formats preserve symbol names, but not the
/// parameter register classes whose drift is dangerous on Win64.
fn extract_ffi_signatures(source: &str) -> Vec<(String, AbiSignature)> {
    const PREFIX: &str = "pub extern \"C\" fn ";
    let mut signatures = Vec::new();
    let mut cursor = 0;

    while let Some(relative_start) = source[cursor..].find(PREFIX) {
        let prefix_start = cursor + relative_start;
        let line_start = source[..prefix_start]
            .rfind('\n')
            .map_or(0, |newline| newline + 1);
        if !source[line_start..prefix_start].trim().is_empty() {
            // Ignore examples and prose such as
            // `// pub extern "C" fn perry_ui_...`.
            cursor = prefix_start + PREFIX.len();
            continue;
        }

        let name_start = prefix_start + PREFIX.len();
        let Some(relative_open) = source[name_start..].find('(') else {
            break;
        };
        let open = name_start + relative_open;
        let name = source[name_start..open].trim();
        if !name.starts_with("perry_ui_") && !name.starts_with("perry_system_") {
            cursor = open + 1;
            continue;
        }

        let mut depth = 0usize;
        let mut close = None;
        for (relative, ch) in source[open..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + relative);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close.unwrap_or_else(|| panic!("unclosed FFI parameter list for {name}"));
        let params = &source[open + 1..close];
        let args = split_top_level(params)
            .into_iter()
            .filter(|param| !param.trim().is_empty())
            .map(|param| {
                let (_, ty) = param
                    .split_once(':')
                    .unwrap_or_else(|| panic!("missing parameter type in {name}: {param}"));
                parse_abi_type(ty)
            })
            .collect();

        let after_params = &source[close + 1..];
        let signature_end = after_params
            .find('{')
            .unwrap_or_else(|| panic!("missing function body for {name}"));
        let return_source = after_params[..signature_end].trim();
        let ret = return_source
            .strip_prefix("->")
            .map(parse_abi_type)
            .unwrap_or(AbiType::Void);
        signatures.push((name.to_string(), AbiSignature { args, ret }));
        cursor = close + 1;
    }

    signatures
}

fn extract_native_crate_signatures(src_dir: &Path) -> HashMap<String, AbiSignature> {
    fn visit(path: &Path, signatures: &mut HashMap<String, AbiSignature>) {
        if path.is_dir() {
            let Ok(entries) = fs::read_dir(path) else {
                return;
            };
            for entry in entries.flatten() {
                visit(&entry.path(), signatures);
            }
            return;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            return;
        }

        let source = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for (name, signature) in extract_ffi_signatures(&source) {
            if let Some(previous) = signatures.insert(name.clone(), signature.clone()) {
                assert_eq!(
                    previous, signature,
                    "conflicting definitions of {name} include different ABI signatures"
                );
            }
        }
    }

    let mut signatures = HashMap::new();
    visit(src_dir, &mut signatures);
    signatures
}

fn codegen_signature(row: &perry_dispatch::MethodRow, has_receiver: bool) -> AbiSignature {
    use perry_dispatch::{ArgKind, ReturnKind};

    let mut args = Vec::with_capacity(row.args.len() + usize::from(has_receiver));
    if has_receiver {
        args.push(AbiType::I64);
    }
    args.extend(row.args.iter().map(|kind| match kind {
        ArgKind::Widget | ArgKind::Str | ArgKind::I64Raw => AbiType::I64,
        ArgKind::F64 | ArgKind::Closure => AbiType::F64,
    }));
    let ret = match row.ret {
        ReturnKind::Widget | ReturnKind::Promise | ReturnKind::Str | ReturnKind::I64AsF64 => {
            AbiType::I64
        }
        ReturnKind::F64 => AbiType::F64,
        ReturnKind::Void => AbiType::Void,
    };
    AbiSignature { args, ret }
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

    for f in FEATURES.iter() {
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

#[test]
fn ffi_signature_parser_ignores_comments_and_handles_multiline_definitions() {
    let source = r#"
// pub extern "C" fn perry_ui_not_a_definition(value: i64) -> i64 {
#[no_mangle]
pub extern "C" fn perry_ui_real(
    widget: i64,
    callback: f64,
) -> i64 {
    0
}
"#;
    assert_eq!(
        extract_ffi_signatures(source),
        vec![(
            "perry_ui_real".to_string(),
            AbiSignature {
                args: vec![AbiType::I64, AbiType::F64],
                ret: AbiType::I64,
            }
        )]
    );
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

/// Every Windows UI export that native codegen can call must use the exact
/// integer/floating-point ABI shape declared in `perry-dispatch`.
///
/// This is stricter than symbol parity. In the Win64 calling convention,
/// changing an argument from `i64` to `f64` (or moving a callback to a
/// different position) changes which positional register the callee reads
/// while leaving the linker-visible symbol unchanged.
#[test]
fn test_windows_codegen_ffi_signatures() {
    use perry_dispatch::{PERRY_UI_INSTANCE_TABLE, PERRY_UI_TABLE};

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let native = extract_native_crate_signatures(&manifest_dir.join("../perry-ui-windows/src"));
    let mut expected: BTreeMap<&str, AbiSignature> = BTreeMap::new();

    for (table, has_receiver) in [(PERRY_UI_TABLE, false), (PERRY_UI_INSTANCE_TABLE, true)] {
        for row in table {
            let signature = codegen_signature(row, has_receiver);
            if let Some(previous) = expected.insert(row.runtime, signature.clone()) {
                assert_eq!(
                    previous, signature,
                    "codegen tables declare conflicting ABI signatures for {}",
                    row.runtime
                );
            }
        }
    }

    let mut mismatches = Vec::new();
    let mut checked = 0usize;
    for (name, expected_signature) in expected {
        let Some(native_signature) = native.get(name) else {
            // Platform-unsupported dispatch rows are allowed to have no
            // Windows export; symbol parity separately checks every feature
            // marked Supported or Stub.
            continue;
        };
        checked += 1;
        if native_signature != &expected_signature {
            mismatches.push(format!(
                "  {name}: codegen {expected_signature}, Windows {native_signature}"
            ));
        }
    }

    assert!(
        checked > 250,
        "unexpectedly checked only {checked} UI exports"
    );
    assert!(
        mismatches.is_empty(),
        "Windows UI exports disagree with native codegen on {} ABI signature(s):\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn test_web() {
    let source = include_str!("../../perry-codegen-js/src/web_runtime.js");
    let symbols = extract_web_symbols(source);
    check_platform("Web", &symbols, |f| f.web, |f| f.web_name.unwrap_or(f.name));
}
