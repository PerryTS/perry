//! `perry electron` — run an existing Electron app natively.
//!
//! Perry is a drop-in Electron replacement: the app's main process is
//! compiled to a native binary with `import … from "electron"` redirected
//! to the in-repo compat shim (`packages/electron`), and the view runs in
//! the OS-native webview. This module locates the app's main entry —
//! including inside packaged macOS `.app` bundles and `.asar` archives —
//! compiles it through the standard `perry run` pipeline, and launches the
//! resulting binary.

mod asar;

use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use super::compile::{CompileArgs, CompileResult};
use super::run::{find_project_root, read_app_metadata};
use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct ElectronArgs {
    /// Path to the Electron app: a directory containing package.json, a
    /// packaged macOS .app bundle, an .asar archive, or a direct
    /// main-process file (.ts/.js).
    pub path: PathBuf,

    /// Arguments passed through to the compiled app
    #[arg(last = true)]
    pub program_args: Vec<String>,
}

/// A resolved Electron app: the main-process entry and the directory that
/// acts as the app root (project root for the compile, cwd for the run).
#[derive(Debug)]
struct ResolvedApp {
    root: PathBuf,
    entry: PathBuf,
}

pub fn run(args: ElectronArgs, format: OutputFormat, use_color: bool, verbose: u8) -> Result<()> {
    let app = resolve_app_path(&args.path, &asar_cache_root(), format)?;
    let shim = find_electron_shim().ok_or_else(|| {
        anyhow!(
            "cannot locate the Perry Electron compat shim (packages/electron/src/index.ts).\n\
             Set PERRY_ELECTRON_SHIM to the shim directory or its src/index.ts file."
        )
    })?;
    if let OutputFormat::Text = format {
        println!("  Electron shim: {}", shim.display());
    }

    let project_root = find_project_root(&app.root);
    let (app_name, bundle_id) = read_app_metadata(&project_root, &app.entry);

    // Same platform defaults as `perry run`: Node host surface, native
    // target, executable output. The one addition is the alias that
    // redirects `electron` imports to the compat shim (an absolute source
    // path — the module resolver's absolute-specifier branch handles it).
    let compile_args = CompileArgs {
        define: Vec::new(),
        package_aliases: vec![format!("electron={}", shim.display())],
        input: app.entry.clone(),
        output: Some(PathBuf::from(&app_name)),
        keep_intermediates: false,
        print_hir: false,
        no_link: false,
        no_codegen: false,
        enable_wasm_runtime: false,
        target: None,
        platform: super::compile::JavaScriptPlatform::Node,
        libc: None,
        march: None,
        app_bundle_id: Some(bundle_id),
        output_type: "executable".to_string(),
        bundle_extensions: None,
        embed: Vec::new(),
        bunfs_root: None,
        asset_module: Vec::new(),
        type_check: false,
        minify: false,
        features: None,
        enable_geisterhand: false,
        geisterhand_port: None,
        minimal_stdlib: false,
        no_auto_optimize: false,
        debug_symbols: false,
        report_size: false,
        function_source: None,
        no_cache: false,
        // `perry electron` has no `--cache-dir` flag; the resolver still
        // honors `PERRY_CACHE_DIR` / perry.toml `[perry] cacheDir` /
        // package.json `perry.cacheDir`.
        cache_dir: None,
        fast_math: false,
        fp_contract: None,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        explain_lowering: false,
        typed_feedback_profile: None,
        typed_feedback_sites: None,
        opt_report: None,
        statepoint_report: None,
        emit_attest: false,
        emit_sandbox: false,
        lockdown: false,
        strict_eval: false,
        strict_dynamic_import: false,
        strict_unimplemented: false,
        min_windows_version: "10".to_string(),
        windows_subsystem: "auto".to_string(),
        p12_keystore: None,
        p12_password: None,
        harmonyos_cert: None,
        harmonyos_profile: None,
        harmonyos_key_alias: None,
        skip_swift_build: false,
        trace: None,
        focus: None,
    };

    let result = super::compile::run(compile_args, format, use_color, verbose)?;
    launch_in_dir(&result, &app.root, &args.program_args, format)
}

/// Launch the compiled binary with the app root as the child cwd, so
/// renderer asset paths written relative to the app directory (the common
/// Electron convention) resolve the same way they did under Electron.
/// Otherwise mirrors `perry run`'s native launch.
fn launch_in_dir(
    result: &CompileResult,
    app_root: &Path,
    program_args: &[String],
    format: OutputFormat,
) -> Result<()> {
    if let OutputFormat::Text = format {
        println!();
        println!("Running {}...", result.output_path.display());
        println!();
    }

    let exe = if result.output_path.is_absolute() {
        result.output_path.clone()
    } else {
        std::env::current_dir()?.join(&result.output_path)
    };
    if !exe.exists() {
        return Err(anyhow!("Compiled executable not found: {}", exe.display()));
    }

    // Execute inside the bundle so Foundation and AppKit see its application
    // identity (a `.app` output path resolves to its CFBundleExecutable).
    let executable = super::run::launch::native_executable_path(&exe)?;
    let status = std::process::Command::new(&executable)
        .args(program_args)
        .current_dir(app_root)
        .status()
        .map_err(|e| anyhow!("Failed to launch {}: {}", exe.display(), e))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// Locate the Electron compat shim (`packages/electron/src/index.ts`).
///
/// Resolution order:
/// 1. `PERRY_ELECTRON_SHIM` — path to the shim root directory or directly to
///    its `src/index.ts`.
/// 2. Walk the ancestors of the running `perry` executable looking for
///    `packages/electron/src/index.ts` — covers `target/{debug,release}/perry`
///    inside the repo checkout, plus `lib/`-style installed layouts one or
///    two levels below the install root.
fn find_electron_shim() -> Option<PathBuf> {
    const SHIM_RELATIVE: &str = "packages/electron/src/index.ts";

    if let Ok(value) = std::env::var("PERRY_ELECTRON_SHIM") {
        let configured = PathBuf::from(value);
        let candidate = if configured.is_dir() {
            configured.join(SHIM_RELATIVE)
        } else {
            configured
        };
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let exe = std::env::current_exe().ok()?;
    let start = exe.parent()?;
    for dir in start.ancestors() {
        for root in [dir.to_path_buf(), dir.join("lib"), dir.join("lib/perry")] {
            let candidate = root.join(SHIM_RELATIVE);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Resolve the user-supplied path to an app entry + app root, handling
/// package.json directories, packaged `.app` bundles, and `.asar` archives
/// (extracted under `cache_root`).
fn resolve_app_path(input: &Path, cache_root: &Path, format: OutputFormat) -> Result<ResolvedApp> {
    let path = absolutize(input);
    if path.is_dir() {
        if path.extension().and_then(|e| e.to_str()) == Some("app") {
            return resolve_app_bundle(&path, cache_root, format);
        }
        if path.join("package.json").is_file() {
            return resolve_package_dir(&path);
        }
        bail!(
            "'{}' is not an Electron app: no package.json, app.asar, or .app bundle found",
            path.display()
        );
    }
    if path.is_file() {
        if path.extension().and_then(|e| e.to_str()) == Some("asar") {
            let root = extract_asar_with_cache(&path, cache_root, format)?;
            return resolve_package_dir(&root);
        }
        return Ok(ResolvedApp {
            root: path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(".")),
            entry: path,
        });
    }
    bail!("'{}' does not exist", path.display());
}

/// A packaged macOS bundle: `Contents/Resources/app.asar` first, then the
/// unpacked `Contents/Resources/app/` directory.
fn resolve_app_bundle(
    bundle: &Path,
    cache_root: &Path,
    format: OutputFormat,
) -> Result<ResolvedApp> {
    let resources = bundle.join("Contents").join("Resources");
    let asar = resources.join("app.asar");
    if asar.is_file() {
        let root = extract_asar_with_cache(&asar, cache_root, format)?;
        return resolve_package_dir(&root);
    }
    let unpacked = resources.join("app");
    if unpacked.join("package.json").is_file() {
        return resolve_package_dir(&unpacked);
    }
    bail!(
        "'{}' is not an Electron app bundle: neither Contents/Resources/app.asar \
         nor Contents/Resources/app/ with a package.json was found",
        bundle.display()
    );
}

/// A directory whose package.json `main` field names the main-process entry
/// (`index.js` when the field is absent), matching Electron's own rule.
fn resolve_package_dir(dir: &Path) -> Result<ResolvedApp> {
    let pkg_path = dir.join("package.json");
    let content = fs::read_to_string(&pkg_path).with_context(|| {
        format!(
            "cannot read {} while resolving the Electron app entry",
            pkg_path.display()
        )
    })?;
    let pkg: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("{} is not valid JSON", pkg_path.display()))?;
    let main = pkg
        .get("main")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("index.js");
    let entry = if Path::new(main).is_absolute() {
        PathBuf::from(main)
    } else {
        dir.join(main)
    };
    if !entry.is_file() {
        bail!(
            "Electron app entry '{}' (from the package.json `main` field) was not found",
            entry.display()
        );
    }
    Ok(ResolvedApp {
        root: dir.to_path_buf(),
        entry,
    })
}

/// Extract an .asar into a directory under `cache_root`, reusing the
/// previous extraction when the source archive is unchanged (packaged `.app`
/// bundles are often signed and read-only, so nothing is written beside the
/// archive).
fn extract_asar_with_cache(
    asar_path: &Path,
    cache_root: &Path,
    format: OutputFormat,
) -> Result<PathBuf> {
    let canonical = asar_path
        .canonicalize()
        .unwrap_or_else(|_| asar_path.to_path_buf());
    let meta = fs::metadata(&canonical)?;
    let mtime_nanos = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let cache_key = format!("{}|{}|{mtime_nanos}", canonical.display(), meta.len());

    let target = cache_root.join(format!("perry-electron-{:016x}", stable_hash(&cache_key)));
    let extracted = asar::extract_cached(&canonical, &target, &cache_key)
        .with_context(|| format!("cannot extract asar archive {}", canonical.display()))?;
    if extracted && matches!(format, OutputFormat::Text) {
        println!("  Extracted {} → {}", canonical.display(), target.display());
    }
    Ok(target)
}

/// Where extracted asar apps are cached: [`default_asar_cache_root`] when
/// creatable, else the system temp dir.
fn asar_cache_root() -> PathBuf {
    let base = default_asar_cache_root();
    if fs::create_dir_all(&base).is_ok() {
        base
    } else {
        std::env::temp_dir()
    }
}

/// The per-user extraction cache (`<user cache dir>/perry/electron-apps`).
/// Deliberately NOT Perry's project cache-dir convention
/// (`node_modules/.cache/perry`): module collection treats any `.js`/`.cjs`
/// file with a `node_modules` path component as npm package code, so an
/// extraction under `node_modules/.cache` made the app's own entry an
/// untrusted package named `.cache` and the compile refused it as runtime
/// JavaScript. Keep the extracted app an ordinary directory tree.
fn default_asar_cache_root() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("perry")
        .join("electron-apps")
}

/// `DefaultHasher::new()` keys are fixed within a std version, which is all
/// a cache-dir name needs.
fn stable_hash(value: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same component test module collection uses to route a `.js`/`.cjs`
    /// file to the npm-package (runtime JavaScript) classification.
    fn has_node_modules_component(path: &Path) -> bool {
        path.components().any(|c| c.as_os_str() == "node_modules")
    }

    fn write_app(dir: &Path, main: Option<&str>) {
        fs::create_dir_all(dir).unwrap();
        let pkg = match main {
            Some(main) => format!("{{ \"name\": \"fixture\", \"main\": \"{main}\" }}"),
            None => "{ \"name\": \"fixture\" }".to_string(),
        };
        fs::write(dir.join("package.json"), pkg).unwrap();
    }

    #[test]
    fn directory_with_package_json_uses_main_field() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        write_app(&app, Some("main.js"));
        fs::write(app.join("main.js"), "console.log(1)").unwrap();

        let resolved = resolve_app_path(&app, dir.path(), OutputFormat::Text).unwrap();
        assert_eq!(resolved.root, app);
        assert_eq!(resolved.entry, app.join("main.js"));
    }

    #[test]
    fn directory_defaults_to_index_js() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        write_app(&app, None);
        fs::write(app.join("index.js"), "console.log(1)").unwrap();

        let resolved = resolve_app_path(&app, dir.path(), OutputFormat::Text).unwrap();
        assert_eq!(resolved.entry, app.join("index.js"));
    }

    #[test]
    fn app_bundle_uses_unpacked_app_dir_without_asar() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("MyApp.app");
        // Unpacked-dir layout: Contents/Resources/app/package.json.
        let unpacked = bundle.join("Contents/Resources/app");
        write_app(&unpacked, Some("main.js"));
        fs::write(unpacked.join("main.js"), "console.log(1)").unwrap();

        let resolved = resolve_app_path(&bundle, dir.path(), OutputFormat::Text).unwrap();
        assert_eq!(resolved.root, unpacked);
    }

    #[test]
    fn app_bundle_prefers_app_asar_over_unpacked_dir() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("MyApp.app");
        let resources = bundle.join("Contents/Resources");
        // Both layouts present: the asar must win.
        let unpacked = resources.join("app");
        write_app(&unpacked, Some("unpacked-entry.js"));
        fs::write(unpacked.join("unpacked-entry.js"), "console.log(2)").unwrap();
        let asar_app = dir.path().join("asar-src");
        write_app(&asar_app, Some("main.js"));
        fs::write(asar_app.join("main.js"), "console.log(1)").unwrap();
        fs::create_dir_all(&resources).unwrap();
        pack_fixture(&asar_app, &resources.join("app.asar"));

        let resolved = resolve_app_path(&bundle, dir.path(), OutputFormat::Text).unwrap();
        assert_eq!(fs::read(&resolved.entry).unwrap(), b"console.log(1)");
    }

    #[test]
    fn direct_file_path_is_used_as_entry() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("main.cjs");
        fs::write(&entry, "console.log(1)").unwrap();

        let resolved = resolve_app_path(&entry, dir.path(), OutputFormat::Text).unwrap();
        assert_eq!(resolved.entry, entry);
        assert_eq!(resolved.root, dir.path());
    }

    #[test]
    fn non_app_directory_errors() {
        let dir = tempfile::tempdir().unwrap();
        let err = resolve_app_path(dir.path(), dir.path(), OutputFormat::Text).unwrap_err();
        assert!(
            err.to_string()
                .contains("is not an Electron app: no package.json, app.asar, or .app bundle"),
            "{err}"
        );
    }

    #[test]
    fn missing_path_errors() {
        let err = resolve_app_path(
            Path::new("/definitely/not/here"),
            &std::env::temp_dir(),
            OutputFormat::Text,
        )
        .unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
    }

    #[test]
    fn main_field_pointing_at_nothing_errors() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        write_app(&app, Some("gone.js"));

        let err = resolve_app_path(&app, dir.path(), OutputFormat::Text).unwrap_err();
        assert!(err.to_string().contains("`main` field"), "{err}");
    }

    #[test]
    fn asar_archive_extracts_to_cache_and_resolves_entry() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        write_app(&app, Some("main.js"));
        fs::write(app.join("main.js"), "console.log(1)").unwrap();
        let asar_path = dir.path().join("app.asar");
        pack_fixture(&app, &asar_path);

        let resolved = resolve_app_path(&asar_path, dir.path(), OutputFormat::Text).unwrap();
        assert_eq!(fs::read(&resolved.entry).unwrap(), b"console.log(1)");
    }

    /// Regression: the extraction cache once lived under Perry's project
    /// cache dir (`node_modules/.cache/perry`), so module collection saw a
    /// `node_modules` component in the extracted entry's path, classified the
    /// app's own `main.cjs` as an untrusted npm package named `.cache`, and
    /// refused the compile as runtime JavaScript.
    #[test]
    fn default_asar_cache_root_is_outside_node_modules() {
        let root = default_asar_cache_root();
        assert!(
            !has_node_modules_component(&root),
            "asar extraction cache {} must not sit under node_modules",
            root.display()
        );
        assert!(root.ends_with("perry/electron-apps"), "{}", root.display());
    }

    #[test]
    fn asar_with_cjs_main_resolves_to_extracted_entry() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        write_app(&app, Some("main.cjs"));
        fs::write(app.join("main.cjs"), "require('electron')").unwrap();
        let asar_path = dir.path().join("app.asar");
        pack_fixture(&app, &asar_path);
        let cache = dir.path().join("cache");

        let resolved = resolve_app_path(&asar_path, &cache, OutputFormat::Text).unwrap();
        assert!(
            resolved.root.starts_with(&cache),
            "{}",
            resolved.root.display()
        );
        assert_eq!(resolved.entry, resolved.root.join("main.cjs"));
        assert_eq!(fs::read(&resolved.entry).unwrap(), b"require('electron')");
        assert!(!has_node_modules_component(&resolved.entry));
    }

    /// Pack a fixture dir into an asar using the same byte layout the
    /// reader consumes (validated against `npx @electron/asar` output).
    fn pack_fixture(source: &Path, dest: &Path) {
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        collect_files(source, source, &mut files);
        files.sort_by(|a, b| a.0.cmp(&b.0));

        let mut root = serde_json::Map::new();
        let mut offset = 0u64;
        for (rel, bytes) in &files {
            root.insert(
                rel.clone(),
                serde_json::json!({
                    "size": bytes.len(),
                    "offset": offset.to_string(),
                }),
            );
            offset += bytes.len() as u64;
        }
        // Flat fixture: all files at the archive root (paths here have no
        // subdirectories), which the JSON tree encodes as plain entries.
        // Four-field header, matching a real `npx @electron/asar` archive.
        let json_bytes = serde_json::to_vec(&serde_json::json!({ "files": root })).unwrap();
        let padding = (4 - (json_bytes.len() % 4)) % 4;
        let json_pickle_size = 4 + json_bytes.len() + padding;
        let header_pickle_size = 4 + json_pickle_size;

        let mut archive = Vec::new();
        archive.extend_from_slice(&4u32.to_le_bytes());
        archive.extend_from_slice(&(header_pickle_size as u32).to_le_bytes());
        archive.extend_from_slice(&(json_pickle_size as u32).to_le_bytes());
        archive.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        archive.extend_from_slice(&json_bytes);
        archive.extend(std::iter::repeat(0u8).take(padding));
        for (_, bytes) in &files {
            archive.extend_from_slice(bytes);
        }
        fs::write(dest, archive).unwrap();
    }

    fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                collect_files(root, &path, out);
            } else {
                out.push((
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    fs::read(&path).unwrap(),
                ));
            }
        }
    }
}
