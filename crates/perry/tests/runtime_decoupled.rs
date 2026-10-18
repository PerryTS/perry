//! The compiler reads no runtime input except the GC effects sidecar.
//!
//! The object cache keys every object on a hash of the perry binary
//! (`object_cache::perry_build_id`) and on the digest of the GC effects table
//! read from the runtime archive's `.gc-effects` sidecar (`gc_effects` key
//! field). A runtime-only edit therefore keeps every cached object exactly
//! when (1) the perry binary is built from no runtime file and (2) codegen
//! reads nothing from the runtime archives but the sidecar. Then a hit serves
//! the object a fresh compile emits. This checks both:
//!
//! - the CLI's dependency graph never reaches perry-runtime (any target);
//! - the files the perry binary's build read (rustc dep-info of the binary
//!   and of each local crate it links, and their build scripts'
//!   `rerun-if-changed` paths) include no file of a runtime-only crate, so no
//!   `include_str!` or build script of codegen reads runtime sources;
//! - before codegen, the pipeline reaches the runtime archives only through
//!   the sidecar reader and the link-set lookups whose paths link after it.
//!
//! A runtime input here is a file of a local crate the runtime builds with
//! and the compiler does not (today: perry-runtime itself).
//!
//! Sabotage checks: make perry-runtime a normal dependency of perry again, or
//! `include_str!` a perry-runtime source outside a test in perry-codegen, or
//! call `find_stdlib_library` before codegen in run_pipeline.rs; each fails.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// `cargo metadata --no-deps`: the workspace packages and their declared
/// dependencies (every target). Only local crates matter here: a registry
/// crate cannot depend on a workspace path crate.
fn metadata(root: &Path) -> serde_json::Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .current_dir(root)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Local package name -> (crate dir, local normal/build dependencies of any
/// target, crate names of its targets).
struct Graph {
    packages: HashMap<String, (PathBuf, Vec<String>, Vec<String>)>,
}

impl Graph {
    fn new(metadata: &serde_json::Value) -> Self {
        let mut packages = HashMap::new();
        for package in metadata["packages"].as_array().unwrap() {
            let dir = PathBuf::from(package["manifest_path"].as_str().unwrap())
                .parent()
                .unwrap()
                .to_path_buf();
            let deps = package["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|dep| !dep["path"].is_null() && dep["kind"].as_str() != Some("dev"))
                .map(|dep| dep["name"].as_str().unwrap().to_string())
                .collect();
            let crates = package["targets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|target| target["name"].as_str().unwrap().replace('-', "_"))
                .collect();
            packages.insert(
                package["name"].as_str().unwrap().to_string(),
                (dir, deps, crates),
            );
        }
        Self { packages }
    }

    /// Every local package reachable from `name` over normal and build
    /// edges, for any target, with the path that first reached it.
    fn closure(&self, name: &str) -> HashMap<String, Vec<String>> {
        let mut reached = HashMap::new();
        let mut pending = vec![(name.to_string(), vec![name.to_string()])];
        while let Some((package, path)) = pending.pop() {
            if reached.contains_key(&package) {
                continue;
            }
            for dep in &self.packages[&package].1 {
                let mut next = path.clone();
                next.push(dep.clone());
                pending.push((dep.clone(), next));
            }
            reached.insert(package, path);
        }
        reached
    }

    fn local_dirs(&self, closure: &HashMap<String, Vec<String>>) -> BTreeSet<PathBuf> {
        closure
            .keys()
            .map(|name| self.packages[name].0.clone())
            .collect()
    }
}

/// Crate dirs the runtime builds from and the compiler does not.
fn runtime_only_dirs(graph: &Graph) -> BTreeSet<PathBuf> {
    let compiler = graph.local_dirs(&graph.closure("perry"));
    graph
        .local_dirs(&graph.closure("perry-runtime"))
        .into_iter()
        .filter(|dir| !compiler.contains(dir))
        .collect()
}

#[test]
fn the_cli_dependency_graph_never_reaches_perry_runtime() {
    let graph = Graph::new(&metadata(&root()));
    if let Some(path) = graph.closure("perry").get("perry-runtime") {
        panic!(
            "perry links perry-runtime through {}: every runtime edit would change the \
             perry binary and miss every cached object. Move what the CLI needs into a \
             crate without runtime code (perry-runtime-manifest, perry-update-manifest), \
             or make it a dev-dependency if only tests need it.",
            path.join(" -> ")
        );
    }
}

/// `path` with `.` and `..` components folded, relative paths taken from `base`.
fn normalize(base: &Path, path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in base.join(path).components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// The rules of a makefile-style dep-info file: (outputs, inputs) of every
/// `output: input input ...` line (the `input:` lines without inputs add
/// nothing).
fn dep_info_rules(text: &str) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let split = |list: &str| -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let mut current = String::new();
        let mut chars = list.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' if chars.peek() == Some(&' ') => {
                    current.push(' ');
                    chars.next();
                }
                ' ' => {
                    if !current.is_empty() {
                        paths.push(PathBuf::from(std::mem::take(&mut current)));
                    }
                }
                _ => current.push(c),
            }
        }
        if !current.is_empty() {
            paths.push(PathBuf::from(current));
        }
        paths
    };
    let (mut outputs, mut inputs) = (Vec::new(), Vec::new());
    for line in text.lines() {
        if let Some((output, deps)) = line.split_once(": ") {
            outputs.extend(split(output));
            inputs.extend(split(deps));
        }
    }
    (outputs, inputs)
}

/// Whether `candidate` is the file `exe` (cargo hard-links the uplifted
/// binary to the copy it built; where it copies, the lengths match).
fn same_file(candidate: &Path, exe: &Path) -> bool {
    let (Ok(a), Ok(b)) = (std::fs::metadata(candidate), std::fs::metadata(exe)) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(not(unix))]
    {
        a.len() == b.len()
    }
}

/// The files under `dir`, `depth` levels deep at most.
fn files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() && depth > 0 => files(&path, depth - 1, out),
            Ok(kind) if kind.is_file() => out.push(path),
            _ => {}
        }
    }
}

/// Every file the build of the perry binary read, as rustc and the build
/// scripts recorded it in the target dir cargo built this test's `perry`
/// into: the dep-info of the binary and of every library build (an rlib, not
/// a test harness) of each local crate in its closure, plus those packages'
/// build-script `rerun-if-changed` paths. Both target layouts: `deps/` +
/// `build/<pkg>-<hash>/output`, and `build/<pkg>/<hash>/{out,run/stdout}`.
fn perry_build_inputs(root: &Path, graph: &Graph, exe: &Path) -> Vec<PathBuf> {
    let profile_dir = exe.parent().unwrap();
    let closure = graph.closure("perry");
    let mut crates: BTreeSet<String> = BTreeSet::new();
    for package in closure.keys() {
        crates.extend(graph.packages[package].2.iter().cloned());
    }
    let is_library_output = |output: &Path| {
        let name = output.file_name().unwrap_or_default().to_string_lossy();
        let Some(stem) = name
            .strip_suffix(".rlib")
            .or_else(|| name.strip_suffix(".rmeta"))
        else {
            return false;
        };
        let stem = stem.strip_prefix("lib").unwrap_or(stem);
        stem.rsplit_once('-')
            .is_some_and(|(krate, _hash)| crates.contains(krate))
    };
    let build_dir = profile_dir.join("build");
    let mut all = Vec::new();
    files(&profile_dir.join("deps"), 0, &mut all);
    files(&build_dir, 3, &mut all);
    let mut inputs = Vec::new();
    let mut found_bin = false;
    for file in &all {
        let name = file.file_name().unwrap().to_string_lossy();
        if name.ends_with(".d") {
            let Ok(text) = std::fs::read_to_string(file) else {
                continue;
            };
            let (outputs, deps) = dep_info_rules(&text);
            let binary = outputs.iter().any(|o| same_file(&normalize(root, o), exe));
            found_bin |= binary;
            if binary || outputs.iter().any(|o| is_library_output(o)) {
                inputs.extend(deps.iter().map(|d| normalize(root, d)));
            }
            continue;
        }
        let build_script_stdout = name == "output"
            || (name == "stdout" && file.parent().is_some_and(|p| p.ends_with("run")));
        if !build_script_stdout {
            continue;
        }
        let Ok(relative) = file.strip_prefix(&build_dir) else {
            continue;
        };
        let first = relative
            .components()
            .next()
            .unwrap()
            .as_os_str()
            .to_string_lossy();
        let package = closure.keys().find(|package| {
            *first == **package
                || first
                    .strip_prefix(package.as_str())
                    .and_then(|rest| rest.strip_prefix('-'))
                    .is_some_and(|hash| {
                        hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit())
                    })
        });
        let Some(package) = package else {
            continue;
        };
        let dir = &graph.packages[package].0;
        let Ok(output) = std::fs::read_to_string(file) else {
            continue;
        };
        for line in output.lines() {
            let path = line
                .strip_prefix("cargo:rerun-if-changed=")
                .or_else(|| line.strip_prefix("cargo::rerun-if-changed="));
            if let Some(path) = path {
                inputs.push(normalize(dir, Path::new(path)));
            }
        }
    }
    assert!(
        found_bin,
        "no dep-info under {} names {}",
        profile_dir.display(),
        exe.display()
    );
    inputs
}

#[test]
fn the_perry_binary_is_built_from_no_runtime_file() {
    let root = root();
    let graph = Graph::new(&metadata(&root));
    let runtime_only = runtime_only_dirs(&graph);
    assert!(
        runtime_only
            .iter()
            .any(|dir| dir.ends_with("perry-runtime")),
        "perry-runtime is in perry's dependency closure (see \
         the_cli_dependency_graph_never_reaches_perry_runtime)"
    );
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let inputs = perry_build_inputs(&root, &graph, &exe);
    // The recorded inputs must cover the path dependencies, or this proves
    // nothing.
    assert!(
        inputs
            .iter()
            .any(|p| p.ends_with("crates/perry-codegen/src/lib.rs")),
        "the recorded build inputs of {} do not list perry-codegen's sources",
        exe.display()
    );
    let offending: BTreeSet<String> = inputs
        .iter()
        .filter(|p| runtime_only.iter().any(|dir| p.starts_with(dir)))
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        offending.is_empty(),
        "the perry binary is built from runtime files, so a runtime edit changes it and \
         misses every cached object. Share such facts through a crate the runtime build \
         reads too (perry-runtime-manifest), or keep the read in a test:\n  {}",
        offending.into_iter().collect::<Vec<_>>().join("\n  ")
    );
}

/// The identifiers in `source` outside `//` comments.
fn identifiers(source: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for line in source.lines() {
        let code = line.split("//").next().unwrap();
        let mut start = None;
        for (i, c) in code
            .char_indices()
            .chain(std::iter::once((code.len(), ' ')))
        {
            let ident = c.is_ascii_alphanumeric() || c == '_';
            match (ident, start) {
                (true, None) => start = Some(i),
                (false, Some(s)) => {
                    out.push(&code[s..i]);
                    start = None;
                }
                _ => {}
            }
        }
    }
    out
}

/// Whether `ident` opens, locates or reads a runtime archive or its sidecar.
fn reaches_runtime_archives(ident: &str) -> bool {
    ((ident.starts_with("find_") || ident.starts_with("locate_")) && ident.ends_with("_library"))
        || ident.starts_with("runtime_library_")
        || ident.starts_with("ensure_runtime_library")
        || matches!(
            ident,
            "find_library"
                | "archive_key"
                | "Sidecar"
                | "sidecar_path"
                | "build_optimized_libs"
                | "compilation_gc_effects"
        )
}

#[test]
fn codegen_reads_the_runtime_only_through_the_sidecar() {
    let pipeline =
        std::fs::read_to_string(root().join("crates/perry/src/commands/compile/run_pipeline.rs"))
            .unwrap();
    let codegen = pipeline
        .find("perry_codegen::compile_module(")
        .expect("the codegen call in run_pipeline.rs");
    let before: BTreeSet<&str> = identifiers(&pipeline[..codegen])
        .into_iter()
        .filter(|ident| reaches_runtime_archives(ident))
        .collect();
    // compilation_gc_effects: the sidecar, checked against the archive bytes;
    // its table's digest is the `gc_effects` object-key field.
    // find_runtime_library: the harmonyos "is a prebuilt runtime on disk"
    // check (a path, no bytes). find_wasm_host_library, build_optimized_libs:
    // the link set, whose paths are consumed by the link after codegen.
    let allowed: BTreeSet<&str> = [
        "compilation_gc_effects",
        "find_runtime_library",
        "find_wasm_host_library",
        "build_optimized_libs",
    ]
    .into();
    let extra: Vec<&&str> = before.difference(&allowed).collect();
    assert!(
        extra.is_empty(),
        "run_pipeline.rs reaches the runtime archives before codegen through {extra:?}. \
         Codegen may read the runtime only through the GC effects sidecar (its digest is \
         in the object key); anything else read from the archives before codegen would \
         let a cached object outlive the runtime it was compiled against."
    );
}
