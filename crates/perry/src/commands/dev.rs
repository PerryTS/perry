//! Dev command - watch TypeScript source and auto-recompile on changes.
//!
//! Usage:
//!   perry dev src/main.ts
//!   perry dev src/server.ts --watch extra/dir -- --port 8080
//!
//! Watches the project tree (rooted at the nearest `package.json` / `perry.toml`,
//! or the entry's parent directory), recompiles on any `.ts` / `.tsx` / `.json`
//! / `.toml` change, kills the previous child process, and relaunches the new
//! binary. Events are debounced over a short window so editor "save storms"
//! trigger a single rebuild.
//!
//! This is the V1 watch mode: it shells out to the existing compile pipeline
//! and relies on Perry's auto-optimize library cache for speed.
//!
//! V2.1 (this file): an in-memory AST cache keyed by canonical path is held
//! across rebuilds in a single `perry dev` session. On a rebuild, unchanged
//! files reuse the previous parsed `swc_ecma_ast::Module` and skip the parse
//! step entirely. Invalidation is content-addressed (full source byte
//! comparison), so editor-format-on-save, timestamp-only touches, and git
//! checkouts all behave correctly. Set `PERRY_DEV_VERBOSE=1` to print the
//! hit/miss counts per rebuild.
//!
//! V2.2 (scoped in issue #131) will add per-module `.o` reuse on disk.

use anyhow::{anyhow, Result};
use clap::Args;
use console::style;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::compile::{CompileArgs, ParseCache};
use crate::OutputFormat;

/// How long to wait after the first event before triggering a rebuild,
/// so that a burst of save events collapses into one build.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// Directory names that are never watched and never trigger rebuilds.
const IGNORED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    "dist",
    "build",
    ".perry-dev",
    // The default cache dir is now `node_modules/.cache/perry`, already
    // covered by the `node_modules` entry above. Kept so a legacy
    // `.perry-cache/` dir (or a `--cache-dir .perry-cache` override) still
    // never triggers a rebuild.
    ".perry-cache",
];

/// File extensions whose changes should trigger a rebuild.
const TRIGGER_EXTS: &[&str] = &["ts", "tsx", "mts", "cts", "json", "toml"];

#[derive(Args, Debug)]
pub struct DevArgs {
    /// Entry TypeScript file
    pub input: PathBuf,

    /// Output executable path (default: .perry-dev/<entry-stem>)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Extra directories to watch (comma-separated or repeated)
    #[arg(long, value_delimiter = ',')]
    pub watch: Vec<PathBuf>,

    /// Arguments to forward to the compiled binary. Place after `--`.
    /// Example: perry dev src/main.ts -- --port 3000
    #[arg(last = true)]
    pub child_args: Vec<String>,
}

pub fn run(args: DevArgs, _format: OutputFormat, use_color: bool, verbose: u8) -> Result<()> {
    let input = args
        .input
        .canonicalize()
        .map_err(|e| anyhow!("cannot resolve entry '{}': {}", args.input.display(), e))?;
    if !input.is_file() {
        return Err(anyhow!("entry is not a file: {}", input.display()));
    }
    if input.extension().and_then(|e| e.to_str()) != Some("ts") {
        return Err(anyhow!("entry must be a .ts file: {}", input.display()));
    }

    let output_path = resolve_output(&args.output, &input)?;

    let entry_dir = input
        .parent()
        .ok_or_else(|| anyhow!("entry has no parent directory"))?
        .to_path_buf();
    let project_root = find_project_root(&entry_dir);

    let mut watch_roots = vec![project_root.clone()];
    for extra in &args.watch {
        match extra.canonicalize() {
            Ok(p) => watch_roots.push(p),
            Err(e) => eprintln!(
                "{} skipping --watch {}: {}",
                paint("!", "yellow", use_color),
                extra.display(),
                e
            ),
        }
    }

    print_banner(&project_root, &input, &output_path, &watch_roots, use_color);

    // Per-session parse cache: kept alive across rebuilds so unchanged files
    // skip parse+lower on the hot path. See module-level docs for details.
    let mut parse_cache = ParseCache::new();
    let verbose_cache = std::env::var("PERRY_DEV_VERBOSE").ok().as_deref() == Some("1");

    // Initial build + spawn.
    let mut child: Option<Child> = match build_once(
        &input,
        &output_path,
        verbose,
        &mut parse_cache,
        verbose_cache,
        use_color,
    ) {
        Ok(()) => spawn_child(&output_path, &args.child_args, use_color).ok(),
        Err(e) => {
            eprintln!(
                "{} initial build failed: {:#}",
                paint("✗", "red", use_color),
                e
            );
            eprintln!("  {}", paint("waiting for changes...", "dim", use_color));
            None
        }
    };

    // Set up the watcher. Events arrive on `rx`.
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher =
        notify::recommended_watcher(tx).map_err(|e| anyhow!("failed to create watcher: {}", e))?;
    for root in &watch_roots {
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|e| anyhow!("failed to watch {}: {}", root.display(), e))?;
    }

    // Main loop: wait for relevant change, debounce, rebuild, relaunch.
    loop {
        // Block until something happens.
        let first = match rx.recv() {
            Ok(ev) => ev,
            Err(_) => break, // sender dropped; exit cleanly
        };
        if !is_relevant(&first) {
            continue;
        }

        // Debounce: swallow any follow-up events within DEBOUNCE.
        let deadline = Instant::now() + DEBOUNCE;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match rx.recv_timeout(remaining) {
                Ok(_) => continue,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    cleanup_child(&mut child);
                    return Ok(());
                }
            }
        }

        eprintln!(
            "{} change detected — rebuilding...",
            paint("⟳", "yellow", use_color)
        );

        // Kill any running child before we rebuild — the binary file is about
        // to be overwritten and a running child would block the link step on
        // some OSes (and race on all of them).
        cleanup_child(&mut child);

        let started = Instant::now();
        match build_once(
            &input,
            &output_path,
            verbose,
            &mut parse_cache,
            verbose_cache,
            use_color,
        ) {
            Ok(()) => {
                let ms = started.elapsed().as_millis();
                eprintln!("{} rebuilt in {}ms", paint("✓", "green", use_color), ms);
                match spawn_child(&output_path, &args.child_args, use_color) {
                    Ok(c) => child = Some(c),
                    Err(e) => {
                        eprintln!("{} failed to launch: {:#}", paint("✗", "red", use_color), e)
                    }
                }
            }
            Err(e) => {
                eprintln!("{} build failed: {:#}", paint("✗", "red", use_color), e);
                eprintln!(
                    "  {}",
                    paint("waiting for next change...", "dim", use_color)
                );
            }
        }
    }

    cleanup_child(&mut child);
    Ok(())
}

fn resolve_output(output: &Option<PathBuf>, input: &Path) -> Result<PathBuf> {
    if let Some(o) = output {
        return Ok(o.clone());
    }
    let cwd = std::env::current_dir().map_err(|e| anyhow!("cannot read cwd: {}", e))?;
    let dir = cwd.join(".perry-dev");
    std::fs::create_dir_all(&dir).map_err(|e| anyhow!("cannot create {}: {}", dir.display(), e))?;
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
    Ok(dir.join(stem))
}

fn find_project_root(start: &Path) -> PathBuf {
    let mut cur = start.to_path_buf();
    loop {
        if cur.join("package.json").is_file() || cur.join("perry.toml").is_file() {
            return cur;
        }
        if !cur.pop() {
            return start.to_path_buf();
        }
    }
}

/// Decide whether a raw watcher event should trigger a rebuild. We accept
/// modify/create/remove events on files whose extension is in `TRIGGER_EXTS`
/// and whose path does not traverse any ignored directory.
fn is_relevant(res: &notify::Result<Event>) -> bool {
    let event = match res {
        Ok(e) => e,
        Err(_) => return false,
    };
    match event.kind {
        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {}
        _ => return false,
    }
    event.paths.iter().any(|p| is_trigger_path(p))
}

fn is_trigger_path(path: &Path) -> bool {
    for comp in path.components() {
        if let Component::Normal(name) = comp {
            if let Some(s) = name.to_str() {
                if IGNORED_DIRS.contains(&s) {
                    return false;
                }
            }
        }
    }
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => TRIGGER_EXTS.contains(&ext),
        None => false,
    }
}

fn build_once(
    input: &Path,
    output: &Path,
    verbose: u8,
    parse_cache: &mut ParseCache,
    verbose_cache: bool,
    use_color: bool,
) -> Result<()> {
    let args = CompileArgs {
        input: input.to_path_buf(),
        output: Some(output.to_path_buf()),
        keep_intermediates: false,
        print_hir: false,
        no_link: false,
        no_codegen: false,
        enable_wasm_runtime: false,
        target: None,
        libc: None,
        app_bundle_id: None,
        output_type: "executable".to_string(),
        bundle_extensions: None,
        embed: Vec::new(),
        type_check: false,
        minify: false,
        features: None,
        enable_geisterhand: false,
        geisterhand_port: None,
        minimal_stdlib: false,
        no_auto_optimize: false,
        debug_symbols: false,
        no_cache: false,
        // `perry dev` has no `--cache-dir` flag of its own; the resolver
        // still honors `PERRY_CACHE_DIR` / perry.toml `[perry] cacheDir` /
        // package.json `perry.cacheDir`.
        cache_dir: None,
        fast_math: false,
        fp_contract: None,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        emit_attest: false,
        emit_sandbox: false,
        lockdown: false,
        strict_eval: false,
        strict_dynamic_import: false,
        strict_unimplemented: false,
        min_windows_version: "10".to_string(),
        windows_subsystem: "auto".to_string(),
        // Phase 2 v7: harmonyos signing flags. `perry dev` is the watch
        // mode for local iteration; unsigned HAPs are fine, fall through
        // to env / saved config.
        p12_keystore: None,
        p12_password: None,
        harmonyos_cert: None,
        harmonyos_profile: None,
        harmonyos_key_alias: None,
        skip_swift_build: false,
        trace: None,
        focus: None,
    };
    parse_cache.reset_counters();
    let result = super::compile::run_with_parse_cache(
        args,
        Some(parse_cache),
        OutputFormat::Text,
        true,
        verbose,
    )?;
    if verbose_cache {
        let hits = parse_cache.hits();
        let misses = parse_cache.misses();
        let total = hits + misses;
        if total > 0 {
            eprintln!(
                "  {} parse cache: {}/{} hit ({} miss)",
                paint("•", "dim", use_color),
                hits,
                total,
                misses
            );
        }
        if let Some((h, m, _stores, _errs)) = result.codegen_cache_stats {
            let total = h + m;
            if total > 0 {
                eprintln!(
                    "  {} codegen cache: {}/{} hit ({} miss)",
                    paint("•", "dim", use_color),
                    h,
                    total,
                    m
                );
            }
        }
    }
    Ok(())
}

fn spawn_child(bin: &Path, child_args: &[String], use_color: bool) -> Result<Child> {
    eprintln!(
        "{} launching {}",
        paint("▶", "cyan", use_color),
        bin.display()
    );
    Command::new(bin)
        .args(child_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| anyhow!("failed to spawn {}: {}", bin.display(), e))
}

fn cleanup_child(child: &mut Option<Child>) {
    if let Some(mut c) = child.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

fn print_banner(
    project_root: &Path,
    input: &Path,
    output: &Path,
    watch_roots: &[PathBuf],
    use_color: bool,
) {
    eprintln!(
        "{} {} watching {}",
        paint("●", "cyan", use_color),
        paint("perry dev", "bold", use_color),
        project_root.display()
    );
    eprintln!("  entry:  {}", input.display());
    eprintln!("  output: {}", output.display());
    if watch_roots.len() > 1 {
        for extra in &watch_roots[1..] {
            eprintln!("  watch:  {}", extra.display());
        }
    }
    eprintln!();
}

fn paint(s: &str, color: &str, enabled: bool) -> String {
    if !enabled {
        return s.to_string();
    }
    let styled = match color {
        "red" => style(s).red().bold(),
        "green" => style(s).green().bold(),
        "yellow" => style(s).yellow().bold(),
        "cyan" => style(s).cyan().bold(),
        "dim" => style(s).dim(),
        "bold" => style(s).bold(),
        _ => style(s),
    };
    styled.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, ModifyKind, RemoveKind};
    use std::fs;

    fn ev(kind: EventKind, path: &str) -> notify::Result<Event> {
        Ok(Event::new(kind).add_path(PathBuf::from(path)))
    }

    #[test]
    fn trigger_path_accepts_ts_family() {
        assert!(is_trigger_path(Path::new("src/main.ts")));
        assert!(is_trigger_path(Path::new("src/App.tsx")));
        assert!(is_trigger_path(Path::new("src/lib.mts")));
        assert!(is_trigger_path(Path::new("src/lib.cts")));
        assert!(is_trigger_path(Path::new("package.json")));
        assert!(is_trigger_path(Path::new("perry.toml")));
    }

    #[test]
    fn trigger_path_rejects_other_extensions() {
        assert!(!is_trigger_path(Path::new("src/main.js")));
        assert!(!is_trigger_path(Path::new("README.md")));
        assert!(!is_trigger_path(Path::new("image.png")));
        assert!(!is_trigger_path(Path::new("no_extension")));
    }

    #[test]
    fn trigger_path_rejects_ignored_dirs() {
        assert!(!is_trigger_path(Path::new("node_modules/pkg/index.ts")));
        assert!(!is_trigger_path(Path::new("target/debug/build.ts")));
        assert!(!is_trigger_path(Path::new(".git/HEAD.ts")));
        assert!(!is_trigger_path(Path::new("project/dist/out.ts")));
        assert!(!is_trigger_path(Path::new(".perry-dev/app.ts")));
        assert!(!is_trigger_path(Path::new(".perry-cache/x.ts")));
        assert!(!is_trigger_path(Path::new("a/build/b/c.ts")));
    }

    #[test]
    fn relevant_accepts_modify_create_remove_of_ts() {
        assert!(is_relevant(&ev(
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            "src/main.ts"
        )));
        assert!(is_relevant(&ev(
            EventKind::Create(CreateKind::File),
            "src/new.ts"
        )));
        assert!(is_relevant(&ev(
            EventKind::Remove(RemoveKind::File),
            "src/gone.ts"
        )));
    }

    #[test]
    fn relevant_rejects_non_trigger_paths_and_kinds() {
        assert!(!is_relevant(&ev(
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            "src/main.js"
        )));
        assert!(!is_relevant(&ev(
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            "node_modules/x.ts"
        )));
        assert!(!is_relevant(&ev(
            EventKind::Access(notify::event::AccessKind::Read),
            "src/main.ts"
        )));
        assert!(!is_relevant(&Err(notify::Error::generic("boom"))));
    }

    #[test]
    fn project_root_finds_package_json() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let nested = root.join("a").join("b").join("c");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("package.json"), "{}").unwrap();
        assert_eq!(find_project_root(&nested), root);
    }

    #[test]
    fn project_root_finds_perry_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let nested = root.join("src");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("perry.toml"), "").unwrap();
        assert_eq!(find_project_root(&nested), root);
    }

    #[test]
    fn project_root_falls_back_to_start_when_no_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let start = tmp.path().canonicalize().unwrap().join("lonely");
        fs::create_dir_all(&start).unwrap();
        assert_eq!(find_project_root(&start), start);
    }
}
