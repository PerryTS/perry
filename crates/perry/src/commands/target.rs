//! `perry target add | list | remove`: cross-target libraries without npm.
//!
//! A cross target's libraries ship as `@perryts/perry-target-<triple>` (see
//! `compile/target_packages.rs`). npm users install that package next to
//! perry. Everyone else runs `perry target add <target>`, which downloads the
//! byte-identical tarball from the GitHub release this perry came from,
//! verifies it against a manifest signed with the CLI-update key (its own
//! signing domain, so a CLI update manifest never passes), and installs it
//! into `~/.perry/targets/<version>/<triple>`. perry never runs npm, and
//! never edits a `node_modules` tree: for an npm-installed target, `remove`
//! prints the npm command instead.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};

use crate::commands::compile::target_packages::{
    self, check_root, discover, npm_install, package_name, perry_home_root, perry_home_targets,
    verify_files, NpmInstall, PerryIdentity, RootSource, TargetRoot, HOST_TRIPLE, TARGET_JSON,
};

/// Where perry's GitHub release assets are downloaded from.
const RELEASE_DOWNLOAD_BASE: &str = "https://github.com/PerryTS/perry/releases/download";

#[derive(Args, Debug)]
pub struct TargetArgs {
    #[command(subcommand)]
    pub command: TargetCommand,
}

#[derive(Subcommand, Debug)]
pub enum TargetCommand {
    /// Install a cross target's libraries into ~/.perry/targets.
    ///
    /// Downloads `perry-target-<triple>-<version>.tgz` from the GitHub
    /// release of this perry and verifies its signed manifest. Nothing is
    /// installed unless every check passes.
    Add(AddArgs),
    /// List the installed target packages (npm and ~/.perry) and their status.
    List(ListArgs),
    /// Remove a target from ~/.perry/targets (never from node_modules).
    Remove(RemoveArgs),
}

#[derive(Args, Debug)]
pub struct AddArgs {
    /// A perry `--target` name (android, ios-simulator, wasi, ...) or a Rust triple.
    pub target: String,
    /// Install this tarball instead of downloading it (offline).
    #[arg(long, requires = "manifest", value_name = "TGZ")]
    pub from: Option<PathBuf>,
    /// The signed `.update.json` manifest of the `--from` tarball.
    #[arg(long, requires = "from", value_name = "JSON")]
    pub manifest: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Re-hash every archive against its target.json.
    #[arg(long)]
    pub verify: bool,
}

#[derive(Args, Debug)]
pub struct RemoveArgs {
    /// A perry `--target` name or a Rust triple.
    pub target: String,
    /// Remove the target for every perry version, not only this one.
    #[arg(long)]
    pub all_versions: bool,
}

pub fn run(args: TargetArgs) -> Result<()> {
    let home = dirs::home_dir().context("cannot determine the home directory")?;
    let exe = std::env::current_exe().ok();
    let me = PerryIdentity::current();
    match args.command {
        TargetCommand::Add(a) => run_add(a, exe.as_deref(), &home, me),
        TargetCommand::List(a) => run_list(a, exe.as_deref(), &home, me),
        TargetCommand::Remove(a) => run_remove(a, exe.as_deref(), &home, me),
    }
}

/// `android` → `aarch64-linux-android`; a triple passes through. Which
/// triples have a package is the release's to answer, not a list here.
fn resolve_triple(target: &str) -> Result<String> {
    if let Some(triple) = target_packages::triple_for(target) {
        return Ok(triple.to_string());
    }
    let triple_shaped = target.split('-').count() >= 2
        && target.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-' || b == b'.'
        });
    if triple_shaped {
        return Ok(target.to_string());
    }
    bail!("unknown target `{target}`: pass a perry --target name (for example android, ios, wasi) or a Rust triple")
}

/// The release asset for a triple's package: the npm tarball, renamed.
pub(crate) fn asset_name(triple: &str, version: &str) -> String {
    format!("perry-target-{triple}-{version}.tgz")
}

/// The GitHub release this binary came from: the tag release CI baked in
/// (nightlies have their own), else the stable `v<version>` tag.
fn release_tag(version: &str) -> String {
    option_env!("PERRY_RELEASE_TAG")
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("v{version}"))
}

fn run_add(args: AddArgs, exe: Option<&Path>, home: &Path, me: PerryIdentity<'_>) -> Result<()> {
    let triple = resolve_triple(&args.target)?;
    if triple == HOST_TRIPLE {
        bail!("{triple} is this perry's own platform; its libraries ship with perry");
    }
    if let Some(root) = discover(&triple, exe, Some(home), me)
        .into_iter()
        .find(TargetRoot::is_accepted)
    {
        println!(
            "{} {} is already installed at {}",
            package_name(&triple),
            me.version,
            root.path.display()
        );
        return Ok(());
    }

    let version_dir = perry_home_targets(home).join(me.version);
    fs::create_dir_all(&version_dir)
        .with_context(|| format!("cannot create {}", version_dir.display()))?;
    let download = tempfile::Builder::new()
        .prefix(".download-")
        .tempdir_in(&version_dir)?;
    let (tarball, manifest) = match (args.from, args.manifest) {
        (Some(from), Some(manifest)) => (
            from,
            fs::read(&manifest).with_context(|| format!("cannot read {}", manifest.display()))?,
        ),
        _ => download_release_asset(&triple, me.version, download.path())?,
    };
    let keys = crate::update_checker::trusted_cli_update_keys()?;
    let key_refs: Vec<(&str, &str)> = keys
        .iter()
        .map(|k| (k.key_id.as_str(), k.public_key.as_str()))
        .collect();
    let dest = install_verified(&tarball, &manifest, &triple, me, &key_refs, &version_dir)?;
    println!(
        "Installed {}@{} at {}",
        package_name(&triple),
        me.version,
        dest.display()
    );
    Ok(())
}

/// Fetch the signed manifest and the tarball it names into `dir`.
fn download_release_asset(triple: &str, version: &str, dir: &Path) -> Result<(PathBuf, Vec<u8>)> {
    let tag = release_tag(version);
    let asset = asset_name(triple, version);
    let client = perry_http_client::Client::with_timeout(Duration::from_secs(600))
        .user_agent(format!("perry/{version}"));
    let manifest_url = format!("{RELEASE_DOWNLOAD_BASE}/{tag}/{asset}.update.json");
    let response = client
        .execute(perry_http_client::Request::get(&manifest_url))
        .with_context(|| format!("cannot download {manifest_url}"))?;
    if response.status == 404 {
        bail!("release {tag} has no {asset}: perry {version} has no target package for {triple}");
    }
    let manifest = response
        .error_for_status()
        .with_context(|| format!("cannot download {manifest_url}"))?
        .body;
    // The tarball URL comes from the manifest, which install_verified checks
    // against the signature before anything is unpacked.
    let parsed: perry_update_manifest::TargetPackageManifest =
        serde_json::from_slice(&manifest).context("target package manifest is malformed")?;
    crate::update_checker::require_https(&parsed.artifact.url, "target package")?;
    let tarball = dir.join(&asset);
    let mut sink = FileSink {
        file: fs::File::create(&tarball)?,
    };
    eprintln!("Downloading {asset} ({} bytes)...", parsed.artifact.size);
    client
        .execute_streaming(
            perry_http_client::Request::get(&parsed.artifact.url),
            &mut sink,
        )
        .with_context(|| format!("cannot download {}", parsed.artifact.url))?
        .error_for_status()
        .with_context(|| format!("cannot download {}", parsed.artifact.url))?;
    sink.file.sync_all()?;
    Ok((tarball, manifest))
}

struct FileSink {
    file: fs::File,
}

impl perry_http_client::BodySink for FileSink {
    fn on_head(&mut self, _status: u16, _len: Option<u64>) -> perry_http_client::Result<()> {
        Ok(())
    }
    fn on_chunk(&mut self, bytes: &[u8]) -> perry_http_client::Result<()> {
        use std::io::Write as _;
        self.file
            .write_all(bytes)
            .map_err(|e| perry_http_client::Error::sink(format!("cannot stage download: {e}")))
    }
}

/// Verify `tarball` against the signed `manifest` and install it as
/// `<version_dir>/<triple>`. Every check runs on a private staging copy; the
/// final rename is the only step that touches the destination, so a failed
/// check leaves nothing installed.
pub(crate) fn install_verified(
    tarball: &Path,
    manifest: &[u8],
    triple: &str,
    me: PerryIdentity<'_>,
    trusted_keys: &[(&str, &str)],
    version_dir: &Path,
) -> Result<PathBuf> {
    let manifest: perry_update_manifest::TargetPackageManifest =
        serde_json::from_slice(manifest).context("target package manifest is malformed")?;
    perry_update_manifest::verify_target_manifest(&manifest, triple, me.version, trusted_keys)
        .context("refusing unauthenticated target package manifest")?;
    let expected = asset_name(triple, me.version);
    if manifest.artifact.name != expected {
        bail!(
            "authenticated manifest names {}, not {expected}",
            manifest.artifact.name
        );
    }
    perry_update_manifest::verify_cli_artifact(tarball, &manifest.artifact)
        .context("refusing target package tarball")?;

    fs::create_dir_all(version_dir)?;
    let staging = tempfile::Builder::new()
        .prefix(&format!(".{triple}-"))
        .tempdir_in(version_dir)?;
    let unpacked = staging.path().join("package");
    extract_package(tarball, &unpacked).context("cannot unpack the target package")?;
    let target_json = check_root(&unpacked, triple, me)
        .map_err(|rejected| anyhow::anyhow!("refusing target package: {}", rejected.reason))?;
    verify_files(&unpacked, &target_json).context("refusing target package")?;

    let dest = version_dir.join(triple);
    if dest.exists() {
        // Only an unusable root gets here (a usable one stopped `add`).
        fs::remove_dir_all(&dest).with_context(|| format!("cannot replace {}", dest.display()))?;
    }
    fs::rename(&unpacked, &dest).with_context(|| format!("cannot install {}", dest.display()))?;
    Ok(dest)
}

/// Unpack an npm tarball's `package/` tree into `dest`: regular files and
/// directories only, every path relative and free of `..`.
fn extract_package(tarball: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::open(tarball)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    fs::create_dir_all(dest)?;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let rel = path
            .strip_prefix("package")
            .with_context(|| format!("{} is outside package/", path.display()))?
            .to_path_buf();
        if rel.as_os_str().is_empty() {
            continue;
        }
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            bail!("unsafe path {} in the target package", path.display());
        }
        let out = dest.join(&rel);
        match entry.header().entry_type() {
            tar::EntryType::Directory => fs::create_dir_all(&out)?,
            tar::EntryType::Regular => {
                if let Some(parent) = out.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut file = fs::File::create(&out)?;
                std::io::copy(&mut entry, &mut file)?;
            }
            other => bail!("{} is a {other:?}, not a file", path.display()),
        }
    }
    if !dest.join(TARGET_JSON).is_file() {
        bail!("the tarball has no package/{TARGET_JSON}");
    }
    Ok(())
}

/// Every target root reachable from this perry, whatever its triple.
fn all_roots(exe: Option<&Path>, home: &Path) -> Vec<(String, PathBuf, RootSource)> {
    let mut out = Vec::new();
    if let Some(exe) = exe {
        let exe = target_packages::real_exe(exe);
        for ancestor in exe.parent().into_iter().flat_map(Path::ancestors) {
            let scope = ancestor.join("node_modules").join("@perryts");
            for (name, path) in dir_entries(&scope) {
                if let Some(triple) = name.strip_prefix(target_packages::PACKAGE_PREFIX) {
                    out.push((triple.to_string(), path, RootSource::Npm));
                }
            }
        }
    }
    for (_, version) in dir_entries(&perry_home_targets(home)) {
        for (triple, path) in dir_entries(&version) {
            if !triple.starts_with('.') {
                out.push((triple, path, RootSource::PerryHome));
            }
        }
    }
    out
}

fn dir_entries(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path())))
        .collect();
    out.sort();
    out
}

fn run_list(args: ListArgs, exe: Option<&Path>, home: &Path, me: PerryIdentity<'_>) -> Result<()> {
    let roots = all_roots(exe, home);
    if roots.is_empty() {
        println!("No target packages installed for perry {}.", me.version);
        return Ok(());
    }
    for (triple, path, source) in roots {
        let source = match source {
            RootSource::Npm => "npm",
            RootSource::PerryHome => "~/.perry",
        };
        let (version, status) = match check_root(&path, &triple, me) {
            Ok(manifest) => {
                let status = if !args.verify {
                    "ok".to_string()
                } else {
                    match verify_files(&path, &manifest) {
                        Ok(()) => "ok (verified)".to_string(),
                        Err(e) => format!("corrupt: {e:#}"),
                    }
                };
                (manifest.perry_version, status)
            }
            Err(rejected) => (
                rejected.found_version.unwrap_or_else(|| "?".into()),
                format!("not used: {}", rejected.reason),
            ),
        };
        println!(
            "{triple}  {version}  {source}  {status}  {}",
            path.display()
        );
    }
    Ok(())
}

fn run_remove(
    args: RemoveArgs,
    exe: Option<&Path>,
    home: &Path,
    me: PerryIdentity<'_>,
) -> Result<()> {
    let triple = resolve_triple(&args.target)?;
    let mut removed = 0;
    let doomed: Vec<PathBuf> = if args.all_versions {
        dir_entries(&perry_home_targets(home))
            .into_iter()
            .map(|(_, version)| version.join(&triple))
            .collect()
    } else {
        vec![perry_home_root(home, me.version, &triple)]
    };
    for dir in doomed {
        if dir.is_dir() {
            fs::remove_dir_all(&dir).with_context(|| format!("cannot remove {}", dir.display()))?;
            println!("Removed {}", dir.display());
            removed += 1;
        }
    }
    // npm owns node_modules: name its command, change nothing.
    for (root_triple, path, source) in all_roots(exe, home) {
        if source != RootSource::Npm || root_triple != triple {
            continue;
        }
        let command = match exe.and_then(npm_install) {
            Some(NpmInstall::Project(dir)) => format!(
                "npm uninstall {}   (in {})",
                package_name(&triple),
                dir.display()
            ),
            _ => format!("npm uninstall -g {}", package_name(&triple)),
        };
        println!(
            "{} is installed by npm at {}; remove it with:\n  {command}",
            package_name(&triple),
            path.display()
        );
        removed += 1;
    }
    if removed == 0 {
        println!("No {} is installed.", package_name(&triple));
    }
    Ok(())
}

// The fixtures sign with ed25519-dalek, a direct dependency only with updater-cli.
// Beside this file, not in `target/`: the repo's .gitignore claims `target/`.
#[cfg(all(test, feature = "updater-cli"))]
#[path = "target_tests.rs"]
mod target_tests;
