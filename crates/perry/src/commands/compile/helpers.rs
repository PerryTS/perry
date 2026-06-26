//! Small standalone helpers extracted from `compile.rs` (pure code move).
//!
//! These were free functions / a tiny artifact struct living at the top of the
//! orchestrator. They are pulled into their own module so the trunk stays small;
//! visibility is widened to `pub(crate)` so the relocated pipeline (and the
//! trunk) can still reach them via `use super::*`.

use super::*;

use anyhow::{anyhow, Result};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::OutputFormat;

/// Error text for a `--target` whose codegen backend was compiled out (#5422).
/// Always defined so the `#[cfg(not(...))]` routing arms can call it; unused in
/// a full-cli build, hence the allow.
#[allow(dead_code)]
pub(crate) fn backend_disabled_msg(target: &str, feature: &str) -> String {
    format!(
        "target '{target}' needs the '{feature}' codegen backend, but this perry \
         was built without it. Rebuild with `--features {feature}` (or the default \
         `full-cli` / `all-codegen-backends`)."
    )
}

pub(crate) struct NativeObjectArtifact {
    pub(crate) path: PathBuf,
    pub(crate) bytes: Option<Vec<u8>>,
    pub(crate) fingerprint: String,
    pub(crate) cleanup_after_link: bool,
    pub(crate) reused_cache_path: bool,
    pub(crate) stored_cache_path: bool,
}

impl NativeObjectArtifact {
    pub(crate) fn materialized_bytes(&self) -> usize {
        self.bytes.as_ref().map_or(0, Vec::len)
    }
}

pub(crate) fn native_object_file_stem(module_name: &str) -> String {
    let mut stem = module_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();

    if stem.is_empty() {
        stem.push('_');
    }

    #[cfg(windows)]
    if is_windows_reserved_file_stem(&stem) {
        stem.push('_');
    }

    stem
}

#[cfg(windows)]
pub(crate) fn is_windows_reserved_file_stem(stem: &str) -> bool {
    let lower = stem.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

pub(crate) fn canonical_class_source_prefix(
    class: &perry_hir::Class,
    class_canonical_path: &HashMap<perry_hir::ClassId, String>,
    project_root: &Path,
    fallback_prefix: &str,
) -> String {
    class_canonical_path
        .get(&class.id)
        .map(|path| compute_module_prefix(path, project_root))
        .unwrap_or_else(|| fallback_prefix.to_string())
}

/// Fold the `--libc <glibc|musl>` flag into the effective `--target` (#4826).
///
/// `--libc musl` upgrades a Linux target to its fully-static musl variant:
/// `linux`/`linux-x86_64`/native-host-default → `linux-musl`, and
/// `linux-aarch64`/`linux-arm64` → `linux-aarch64-musl`. It is a no-op for an
/// already-musl target. `glibc`/`gnu` (or no flag) leave the target untouched.
/// `--libc musl` against a non-Linux target is a hard error rather than a
/// silently-ignored flag.
pub(crate) fn apply_libc_to_target(
    target: Option<String>,
    libc: Option<&str>,
) -> Result<Option<String>> {
    let libc = match libc {
        None => return Ok(target),
        Some(l) => l.trim().to_ascii_lowercase(),
    };
    match libc.as_str() {
        // Default / explicit glibc: nothing to do.
        "glibc" | "gnu" | "" => Ok(target),
        "musl" => match target.as_deref() {
            // Default (native host) or explicit x86_64 Linux → x86_64 musl.
            None | Some("linux") | Some("linux-x86_64") => Ok(Some("linux-musl".to_string())),
            Some("linux-aarch64") | Some("linux-arm64") => {
                Ok(Some("linux-aarch64-musl".to_string()))
            }
            // Already a musl target — idempotent.
            Some("linux-musl") | Some("linux-x86_64-musl") | Some("linux-aarch64-musl") => {
                Ok(target)
            }
            Some(other) => anyhow::bail!(
                "--libc musl only applies to Linux targets, but --target is \
                 '{other}'. Drop --libc musl, or build a Linux target \
                 (e.g. --target linux)."
            ),
        },
        other => {
            anyhow::bail!("unknown --libc value '{other}'. Supported: glibc (default) or musl.")
        }
    }
}

pub(crate) fn object_cache_project_root(input: &Path, fallback_project_root: &Path) -> PathBuf {
    let input_parent = input
        .canonicalize()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));

    if let Some(mut dir) = input_parent.clone() {
        loop {
            if dir.join("package.json").exists() || dir.join("perry.toml").exists() {
                return dir;
            }
            if !dir.pop() {
                break;
            }
        }
    }

    if let (Some(input_parent), Ok(cwd)) = (input_parent, std::env::current_dir()) {
        let cwd = cwd.canonicalize().unwrap_or(cwd);
        if input_parent.starts_with(&cwd) {
            return cwd;
        }
    }

    fallback_project_root.to_path_buf()
}

/// #5206 / #5230: print the end-of-compile notice for ahead-of-time-unsupported
/// sites (runtime-unknown `eval(...)` / `new Function(...)`, and non-resolvable
/// dynamic `import(...)`) that were compiled to deferred runtime errors. Drains
/// the shared process-global sink (so re-running a compile in the same process
/// starts fresh) and prints a single stand-out block. No-op when there are no
/// such sites or for JSON output.
pub(crate) fn print_deferred_eval_notice(format: OutputFormat) {
    let sites = perry_hir::take_deferred_eval_sites();
    if sites.is_empty() || !matches!(format, OutputFormat::Text) {
        return;
    }
    // Sort for deterministic output (kind then location).
    let mut sites = sites;
    sites.sort_by(|a, b| (&a.kind, &a.location).cmp(&(&b.kind, &b.location)));
    let n = sites.len();
    let plural = if n == 1 { "site" } else { "sites" };
    // ANSI yellow + bold so the notice stands out from the surrounding build
    // log; degrade to plain text when stderr isn't a TTY.
    let tty = std::io::IsTerminal::is_terminal(&std::io::stderr());
    let (y, b, r) = if tty {
        ("\x1b[33m", "\x1b[1m", "\x1b[0m")
    } else {
        ("", "", "")
    };
    eprintln!();
    eprintln!(
        "{y}{b}notice:{r}{y} {n} ahead-of-time-unsupported {plural} compiled to a deferred runtime error (throws only if reached):{r}"
    );
    // Align the locations into a column for readability.
    let kind_width = sites.iter().map(|s| s.kind.len()).max().unwrap_or(0);
    for s in &sites {
        eprintln!(
            "  - {:<width$}   {}",
            s.kind,
            s.location,
            width = kind_width
        );
    }
    eprintln!(
        "  Pass {b}--strict-eval{r}/{b}--strict-dynamic-import{r} (or set {b}perry.strict = true{r}) to make these a compile-time error instead."
    );
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_class_source_prefix_prefers_defining_path() {
        let class = perry_hir::Class {
            id: 7,
            name: "Observable".to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: Vec::new(),
            constructor: None,
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            computed_members: Vec::new(),
            decorators: Vec::new(),
            is_exported: true,
            is_nested: false,
            aliases: Vec::new(),
        };
        let project_root = PathBuf::from("/repo");
        let mut class_canonical_path = HashMap::new();
        class_canonical_path.insert(
            class.id,
            "/repo/node_modules/rxjs/src/internal/Observable.ts".to_string(),
        );

        assert_eq!(
            canonical_class_source_prefix(
                &class,
                &class_canonical_path,
                &project_root,
                "node_modules_rxjs_src_index_ts",
            ),
            "node_modules_rxjs_src_internal_Observable_ts"
        );
    }

    #[test]
    fn native_object_file_stem_sanitizes_module_names() {
        assert_eq!(
            native_object_file_stem("table-parser/lib/index"),
            "table_parser_lib_index"
        );
        assert_eq!(native_object_file_stem("///"), "_");
    }

    #[cfg(windows)]
    #[test]
    fn native_object_file_stem_avoids_windows_reserved_names() {
        assert_eq!(native_object_file_stem("con"), "con_");
        assert_eq!(
            native_object_file_stem("connected-domain"),
            "connected_domain"
        );
        assert_eq!(native_object_file_stem("aux"), "aux_");
        assert_eq!(native_object_file_stem("COM1"), "COM1_");
    }

    #[test]
    fn object_cache_root_prefers_package_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(dir.path().join("package.json"), "{}\n").unwrap();
        let input = src.join("main.ts");
        std::fs::write(&input, "console.log(1);\n").unwrap();

        assert_eq!(
            object_cache_project_root(&input, &src),
            dir.path().canonicalize().unwrap()
        );
    }
}
