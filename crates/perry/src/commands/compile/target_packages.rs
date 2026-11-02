//! Target and ext packages: one triple's prebuilt libraries, installed apart
//! from perry itself.
//!
//! A package is `@perryts/perry-<kind>-<triple>`, the name derived from the
//! triple [`rust_target_triple`](super::rust_target_triple) returns (or, for
//! perry's own platform, the triple it was built for). There are two kinds:
//!
//! - a **target** package (`perry-target-<triple>`) holds a cross triple's
//!   runtime, stdlib and UI archives (+ the runtime's `.gc-effects` sidecar);
//! - an **ext** package (`perry-ext-<triple>`) holds the triple's
//!   native-extension archives (`libperry_ext_*.a`, the host functions behind
//!   node:http, ws, zlib, ...). They are too big for the host package, so an
//!   npm install fetches them only when a program needs one (see
//!   `commands::target::ensure_ext_package`).
//!
//! Which triples have a package of each kind is one list,
//! `npm/packaged-triples.json`, which the publish scripts stage from too.
//!
//! Both kinds have the same layout: `target.json` and `lib/<archive>[.zst]`,
//! found in two places:
//!
//! - an npm install: `A/node_modules/@perryts/perry-target-<triple>/` for an
//!   ancestor `A` of the perry binary's directory, nearest first. This is
//!   Node's own resolution seen from the binary, so it covers a global
//!   install (unix `<prefix>/lib`, Windows `%APPDATA%\npm`), a project
//!   install (hoisted or nested) and pnpm (the real `.pnpm/...` path of the
//!   binary has the project as an ancestor);
//! - `~/.perry/<targets|ext>/<perry-version>/<triple>/`, written by
//!   `perry target add [--ext]` and by the ext auto-fetch.
//!
//! A root counts only when its `target.json` names this perry's exact
//! version, its source revision (when this binary was built with one) and
//! the requested triple. A root that fails is never used "close enough"; it
//! is reported so the missing-target error can say what was found instead.
//! [`collect_library_candidates`](super::library_search::collect_library_candidates)
//! adds `<root>/lib/<name>` for every accepted root after the install-layout
//! candidates, so zips, Homebrew and workspace builds keep their precedence.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The manifest at a target package's root.
pub(crate) const TARGET_JSON: &str = "target.json";

/// npm scope directory of every package.
pub(crate) const NPM_SCOPE: &str = "@perryts";

/// A triple's two packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackageKind {
    /// `perry-target-<triple>`: a cross triple's runtime, stdlib and UI.
    Target,
    /// `perry-ext-<triple>`: the triple's native-extension archives.
    Ext,
}

impl PackageKind {
    pub(crate) const ALL: [PackageKind; 2] = [PackageKind::Target, PackageKind::Ext];

    /// The npm name prefix after the scope, and the release asset prefix.
    pub(crate) fn prefix(self) -> &'static str {
        match self {
            PackageKind::Target => "perry-target-",
            PackageKind::Ext => "perry-ext-",
        }
    }

    /// The `~/.perry` directory that holds this kind, per version.
    pub(crate) fn home_dir(self) -> &'static str {
        match self {
            PackageKind::Target => "targets",
            PackageKind::Ext => "ext",
        }
    }
}

/// `(Target, aarch64-linux-android)` →
/// `@perryts/perry-target-aarch64-linux-android`.
pub(crate) fn package_name(kind: PackageKind, triple: &str) -> String {
    format!("{NPM_SCOPE}/{}{triple}", kind.prefix())
}

/// The one list of packaged triples, shared with the publish scripts
/// (`scripts/publish/constants.mts` reads the same file, and stage-npm.sh
/// stages exactly these packages).
const PACKAGED_TRIPLES_JSON: &str = include_str!("../../../../../npm/packaged-triples.json");

#[derive(Deserialize)]
struct PackagedTriples {
    targets: Vec<String>,
    ext: Vec<String>,
}

/// Whether perry releases publish a `kind` package for `triple`. A triple
/// with none (a windows-msvc cross, a musl host's extensions) keeps the
/// generic missing-library errors instead of naming a package that does not
/// exist, and is never fetched.
pub(crate) fn is_packaged(kind: PackageKind, triple: &str) -> bool {
    let list: PackagedTriples = serde_json::from_str(PACKAGED_TRIPLES_JSON)
        .expect("npm/packaged-triples.json is checked by the publish tests");
    let triples = match kind {
        PackageKind::Target => list.targets,
        PackageKind::Ext => list.ext,
    };
    triples.iter().any(|t| t == triple)
}

/// The Rust triple of a perry `--target` name, for code outside `compile`.
pub(crate) fn triple_for(target: &str) -> Option<&'static str> {
    super::rust_target_triple(Some(target))
}

/// The triple this perry binary itself runs on (build.rs `TARGET`). Its
/// libraries ship with perry, so it never resolves through a target package.
pub(crate) const HOST_TRIPLE: &str = env!("PERRY_HOST_TRIPLE");

/// `target.json`: the WASI bundle manifest schema, with package paths.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TargetJson {
    pub(crate) schema_version: u32,
    pub(crate) perry_version: String,
    pub(crate) source_revision: String,
    pub(crate) target_triple: String,
    pub(crate) files: Vec<TargetFile>,
}

/// One archive: its path in the package and the hash of its uncompressed bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TargetFile {
    pub(crate) path: String,
    pub(crate) sha256: String,
    pub(crate) size: u64,
}

/// What a root must match: this perry's version and, when built with one,
/// its source revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PerryIdentity<'a> {
    pub(crate) version: &'a str,
    pub(crate) revision: Option<&'a str>,
}

impl PerryIdentity<'static> {
    /// This binary. Release CI sets `PERRY_SOURCE_REVISION` to the commit it
    /// builds; a local build has none and checks the version only.
    pub(crate) fn current() -> Self {
        PerryIdentity {
            version: env!("CARGO_PKG_VERSION"),
            revision: option_env!("PERRY_SOURCE_REVISION").filter(|r| !r.is_empty()),
        }
    }
}

/// Where a root was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RootSource {
    Npm,
    PerryHome,
}

/// A target package root found in search order, and whether it is usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TargetRoot {
    pub(crate) path: PathBuf,
    pub(crate) source: RootSource,
    /// `Ok` with the manifest, or why the root was rejected.
    pub(crate) status: std::result::Result<TargetJson, Rejected>,
}

/// A rejected root: the reason, and the version it declares when readable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rejected {
    pub(crate) found_version: Option<String>,
    pub(crate) reason: String,
}

impl TargetRoot {
    pub(crate) fn is_accepted(&self) -> bool {
        self.status.is_ok()
    }
}

/// `~/.perry/targets` or `~/.perry/ext`.
pub(crate) fn perry_home_kind(home: &Path, kind: PackageKind) -> PathBuf {
    home.join(".perry").join(kind.home_dir())
}

/// `~/.perry/<targets|ext>/<version>/<triple>`.
pub(crate) fn perry_home_root(
    home: &Path,
    kind: PackageKind,
    version: &str,
    triple: &str,
) -> PathBuf {
    perry_home_kind(home, kind).join(version).join(triple)
}

/// The binary's real path: pnpm and npm's global bin link to it, and the
/// ancestors that matter are those of the real file.
pub(crate) fn real_exe(exe: &Path) -> PathBuf {
    fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf())
}

/// Every existing `kind` package directory for `triple`, in search order.
pub(crate) fn root_dirs(
    kind: PackageKind,
    triple: &str,
    exe: Option<&Path>,
    home: Option<&Path>,
    version: &str,
) -> Vec<(PathBuf, RootSource)> {
    let mut out: Vec<(PathBuf, RootSource)> = Vec::new();
    if let Some(exe) = exe {
        let exe = real_exe(exe);
        for ancestor in exe.parent().into_iter().flat_map(Path::ancestors) {
            let dir = ancestor
                .join("node_modules")
                .join(NPM_SCOPE)
                .join(format!("{}{triple}", kind.prefix()));
            if dir.is_dir() && !out.iter().any(|(seen, _)| *seen == dir) {
                out.push((dir, RootSource::Npm));
            }
        }
    }
    if let Some(home) = home {
        let dir = perry_home_root(home, kind, version, triple);
        if dir.is_dir() {
            out.push((dir, RootSource::PerryHome));
        }
    }
    out
}

/// Read `<root>/target.json` and check it against this perry and `triple`.
/// Only the identity is checked here (it runs on every library lookup);
/// `perry target add` and `perry target list --verify` also hash the files.
pub(crate) fn check_root(
    root: &Path,
    triple: &str,
    me: PerryIdentity<'_>,
) -> std::result::Result<TargetJson, Rejected> {
    let path = root.join(TARGET_JSON);
    let manifest: TargetJson = fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
        .map_err(|e| Rejected {
            found_version: None,
            reason: format!("unreadable {}: {e}", path.display()),
        })?;
    let reject = |reason: String| Rejected {
        found_version: Some(manifest.perry_version.clone()),
        reason,
    };
    if manifest.schema_version != 1 {
        return Err(reject(format!(
            "target.json schema {} is not 1",
            manifest.schema_version
        )));
    }
    if manifest.target_triple != triple {
        return Err(reject(format!(
            "it holds {}, not {triple}",
            manifest.target_triple
        )));
    }
    if manifest.perry_version != me.version {
        return Err(reject(format!(
            "it is {}; perry is {}",
            manifest.perry_version, me.version
        )));
    }
    if let Some(revision) = me.revision {
        if manifest.source_revision != revision {
            return Err(reject(format!(
                "it was built from {}; perry {} was built from {revision}",
                short(&manifest.source_revision),
                me.version,
            )));
        }
    }
    Ok(manifest)
}

fn short(revision: &str) -> &str {
    revision.get(..12).unwrap_or(revision)
}

/// Every `kind` root for `triple` in search order, each with its status.
pub(crate) fn discover(
    kind: PackageKind,
    triple: &str,
    exe: Option<&Path>,
    home: Option<&Path>,
    me: PerryIdentity<'_>,
) -> Vec<TargetRoot> {
    root_dirs(kind, triple, exe, home, me.version)
        .into_iter()
        .map(|(path, source)| {
            let status = check_root(&path, triple, me);
            TargetRoot {
                path,
                source,
                status,
            }
        })
        .collect()
}

/// How the binary was installed by npm, for the install hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NpmInstall {
    /// A global prefix: `npm install -g`.
    Global,
    /// A project: `npm install --save-dev`, in this directory.
    Project(PathBuf),
}

/// The npm install that holds this binary, if any. The `node_modules` that
/// holds `@perryts/perry` decides: with a `package.json` beside it, it is a
/// project; without one, a global prefix.
pub(crate) fn npm_install(exe: &Path) -> Option<NpmInstall> {
    let exe = real_exe(exe);
    let node_modules: Vec<&Path> = exe
        .ancestors()
        .filter(|a| a.file_name().is_some_and(|n| n == "node_modules"))
        .collect();
    let holder = node_modules
        .iter()
        .find(|nm| nm.join(NPM_SCOPE).join("perry").is_dir())
        .or(node_modules.last())?;
    let parent = holder.parent()?;
    if parent.join("package.json").is_file() {
        Some(NpmInstall::Project(parent.to_path_buf()))
    } else {
        Some(NpmInstall::Global)
    }
}

/// The error for a cross target with no usable libraries: what to install,
/// pinned to this exact version, and what was found instead.
pub(crate) fn missing_target_message(
    target_name: &str,
    triple: &str,
    exe: Option<&Path>,
    home: Option<&Path>,
    me: PerryIdentity<'_>,
) -> String {
    format!(
        "perry {} has no libraries for target `{target_name}` ({triple}).\n{}",
        me.version,
        install_hint(
            PackageKind::Target,
            triple,
            &format!("perry target add {target_name}"),
            exe,
            home,
            me
        )
    )
}

/// "Install them:" with the pinned npm command (when perry runs from an npm
/// install), the `perry target add` command, and every root that was found
/// but rejected, with why.
pub(crate) fn install_hint(
    kind: PackageKind,
    triple: &str,
    perry_command: &str,
    exe: Option<&Path>,
    home: Option<&Path>,
    me: PerryIdentity<'_>,
) -> String {
    let package = format!("{}@{}", package_name(kind, triple), me.version);
    let mut out = String::from("Install them:\n");
    match exe.and_then(npm_install) {
        Some(NpmInstall::Global) => {
            out.push_str(&format!("  npm install -g {package}\nor, without npm:\n"));
        }
        Some(NpmInstall::Project(dir)) => {
            out.push_str(&format!(
                "  npm install --save-dev {package}   (in {})\nor, without npm:\n",
                dir.display()
            ));
        }
        None => {}
    }
    out.push_str(&format!("  {perry_command}\n"));
    for root in discover(kind, triple, exe, home, me) {
        if let Err(rejected) = &root.status {
            let found = rejected
                .found_version
                .as_deref()
                .map(|v| format!(" {v}"))
                .unwrap_or_default();
            out.push_str(&format!(
                "found {}{found} at {}, not used: {}\n",
                package_name(kind, triple),
                root.path.display(),
                rejected.reason
            ));
        }
    }
    out
}

/// Hash every file `target.json` lists. An archive may be stored raw or as
/// `<path>.zst`; the hash is of the uncompressed bytes either way. Every
/// file under `lib/` must be listed.
pub(crate) fn verify_files(root: &Path, manifest: &TargetJson) -> Result<()> {
    let mut listed = Vec::new();
    for file in &manifest.files {
        let rel = Path::new(&file.path);
        if rel.is_absolute()
            || rel.components().count() != 2
            || !rel.starts_with("lib")
            || rel
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            bail!("target.json lists {:?}, which is not lib/<file>", file.path);
        }
        let raw = root.join(rel);
        let compressed = super::compressed_libs::compressed_sibling(&raw);
        let (stored, reader): (PathBuf, Box<dyn Read>) = if raw.is_file() {
            (raw.clone(), Box::new(fs::File::open(&raw)?))
        } else if compressed.is_file() {
            let input = fs::File::open(&compressed)?;
            (
                compressed.clone(),
                Box::new(zstd::Decoder::new(input).context("init zstd decoder")?),
            )
        } else {
            bail!("{} is missing", raw.display());
        };
        let (digest, size) = sha256_reader(reader)
            .with_context(|| format!("failed to read {}", stored.display()))?;
        if size != file.size || perry_hex::encode(digest) != file.sha256 {
            bail!(
                "{} does not match target.json (size {size}, expected {})",
                stored.display(),
                file.size
            );
        }
        listed.push(stored);
    }
    let lib = root.join("lib");
    for entry in fs::read_dir(&lib).with_context(|| format!("cannot read {}", lib.display()))? {
        let path = entry?.path();
        if !listed.contains(&path) {
            bail!("{} is not listed in target.json", path.display());
        }
    }
    Ok(())
}

fn sha256_reader(mut reader: impl Read) -> std::io::Result<([u8; 32], u64)> {
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }
    Ok((hasher.finalize().into(), size))
}

#[cfg(test)]
mod target_packages_tests;
