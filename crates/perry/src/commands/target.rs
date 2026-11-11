//! `perry target add | list | remove`: target and ext packages without npm.
//!
//! A cross target's runtime, stdlib and UI travel together. Host UI,
//! WebAssembly host and native extensions each have their own signed package.
//! All kinds use the same installer and signature/hash checks, whether fetched
//! on first use, installed beside perry with npm, or added ahead of time by
//! `perry target add` with `--ui`, `--wasm-host` or `--ext`.

use std::fs;
use std::io::IsTerminal as _;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};

use crate::commands::compile::target_packages::{
    self, check_root, discover, install_hint, is_packaged, npm_install, package_name,
    perry_home_kind, perry_home_root, verify_files, NpmInstall, PackageKind, PerryIdentity,
    RootSource, TargetRoot, HOST_TRIPLE, NPM_SCOPE, TARGET_JSON,
};

/// Where perry's GitHub release assets are downloaded from.
const RELEASE_DOWNLOAD_BASE: &str = "https://github.com/PerryTS/perry/releases/download";

/// A directory, or an https URL, holding this release's package assets
/// (`<asset>.tgz` and `<asset>.tgz.update.json`) in place of the GitHub
/// release: an internal mirror, or a test release. The signature check is
/// the same either way.
pub(crate) const RELEASE_MIRROR_ENV: &str = "PERRY_RELEASE_MIRROR";

/// Set to `1` to stop a compile from downloading a missing ext or target
/// package (offline machines, hermetic CI); the compile then fails with the
/// install commands instead.
pub(crate) const NO_AUTO_FETCH_ENV: &str = "PERRY_NO_AUTO_FETCH";

#[derive(Args, Debug)]
pub struct TargetArgs {
    #[command(subcommand)]
    pub command: TargetCommand,
}

#[derive(Subcommand, Debug)]
pub enum TargetCommand {
    /// Install a cross target's libraries, or one host kind selected with
    /// --ui, --wasm-host or --ext, into ~/.perry. A compile fetches missing
    /// packages automatically unless PERRY_NO_AUTO_FETCH=1.
    ///
    /// Downloads this perry version's package from its GitHub release and
    /// verifies the signed manifest and every archive before installation.
    Add(AddArgs),
    /// List the installed library packages (npm and ~/.perry) and their status.
    List(ListArgs),
    /// Remove a library package from ~/.perry (never from node_modules).
    Remove(RemoveArgs),
}

#[derive(Args, Debug)]
pub struct AddArgs {
    /// A perry `--target` name (android, ios-simulator, wasi, ...) or a Rust
    /// triple. With --ui, --wasm-host or --ext it defaults to this perry's own platform.
    #[arg(required_unless_present_any = ["ext", "ui", "wasm_host"])]
    pub target: Option<String>,
    /// Install native extensions (node:http, ws, zlib, ...) instead of target libraries.
    #[arg(long)]
    pub ext: bool,
    /// Select this host's UI libraries.
    #[arg(long, conflicts_with_all = ["ext", "wasm_host"])]
    pub ui: bool,
    /// Select this host's WebAssembly host library.
    #[arg(long, conflicts_with_all = ["ext", "ui"])]
    pub wasm_host: bool,
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
    /// A perry `--target` name or a Rust triple. With --ui, --wasm-host or --ext it defaults to
    /// this perry's own platform.
    #[arg(required_unless_present_any = ["ext", "ui", "wasm_host"])]
    pub target: Option<String>,
    /// Remove native extensions instead of target libraries.
    #[arg(long)]
    pub ext: bool,
    /// Select this host's UI libraries.
    #[arg(long, conflicts_with_all = ["ext", "wasm_host"])]
    pub ui: bool,
    /// Select this host's WebAssembly host library.
    #[arg(long, conflicts_with_all = ["ext", "ui"])]
    pub wasm_host: bool,
    /// Remove the package for every perry version, not only this one.
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

/// `android` → `aarch64-linux-android`; a triple passes through.
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

/// The package a command line names: `<target>` is a target package, `--ext
/// [<target>]` the ext package of that triple (this platform by default).
fn resolve_package(target: Option<&str>, ext: bool) -> Result<(PackageKind, String)> {
    match (target, ext) {
        (Some(target), false) => Ok((PackageKind::Target, resolve_triple(target)?)),
        (Some(target), true) => Ok((PackageKind::Ext, resolve_triple(target)?)),
        (None, true) => Ok((PackageKind::Ext, HOST_TRIPLE.to_string())),
        (None, false) => bail!("name a target (for example android) or pass --ext"),
    }
}

/// The `perry target` command that installs a package, for messages.
fn add_command(kind: PackageKind, target: &str) -> String {
    match kind {
        PackageKind::Target => format!("perry target add {target}"),
        PackageKind::Ext if target == HOST_TRIPLE => "perry target add --ext".to_string(),
        PackageKind::Ext => format!("perry target add --ext {target}"),
        PackageKind::Ui | PackageKind::WasmHost => {
            let flag = kind.home_dir();
            if target == HOST_TRIPLE {
                format!("perry target add --{flag}")
            } else {
                format!("perry target add --{flag} {target}")
            }
        }
    }
}

/// The release asset for a package: the npm tarball, renamed.
pub(crate) fn asset_name(kind: PackageKind, triple: &str, version: &str) -> String {
    format!("{}{triple}-{version}.tgz", kind.prefix())
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
    let (kind, triple) = if args.ui || args.wasm_host {
        (
            if args.ui {
                PackageKind::Ui
            } else {
                PackageKind::WasmHost
            },
            args.target
                .as_deref()
                .map(resolve_triple)
                .transpose()?
                .unwrap_or_else(|| HOST_TRIPLE.to_string()),
        )
    } else {
        resolve_package(args.target.as_deref(), args.ext)?
    };
    if kind == PackageKind::Target && triple == HOST_TRIPLE {
        bail!("{triple} is this perry's own platform; its libraries ship with perry");
    }
    if let Some(root) = discover(kind, &triple, exe, Some(home), me)
        .into_iter()
        .find(TargetRoot::is_accepted)
    {
        println!(
            "{} {} is already installed at {}",
            package_name(kind, &triple),
            me.version,
            root.path.display()
        );
        return Ok(());
    }
    let source = match (args.from, args.manifest) {
        (Some(tarball), Some(manifest)) => Source::Files { tarball, manifest },
        _ => {
            if !is_packaged(kind, &triple) {
                bail!(
                    "perry publishes no {} for {triple}; build the libraries from source or set PERRY_LIB_DIR",
                    package_name(kind, &triple)
                );
            }
            Source::Release
        }
    };
    let keys = compiled_keys()?;
    let dest = install_package(kind, &triple, me, home, source, &key_refs(&keys))
        .map_err(|e| with_unreachable_advice(e, kind, &triple, me, exe))?;
    println!(
        "Installed {}@{} at {}",
        package_name(kind, &triple),
        me.version,
        dest.display()
    );
    Ok(())
}

/// Where `install_package` gets the tarball and its signed manifest.
pub(crate) enum Source {
    /// Local files (`--from/--manifest`).
    Files { tarball: PathBuf, manifest: PathBuf },
    /// This perry's GitHub release, or `PERRY_RELEASE_MIRROR`.
    Release,
}

/// The keyring compiled into this perry, as `(key id, public key)`. Callers
/// read it before fetching anything: a build without one never downloads.
fn compiled_keys() -> Result<Vec<(String, String)>> {
    Ok(crate::update_checker::trusted_cli_update_keys()?
        .into_iter()
        .map(|k| (k.key_id, k.public_key))
        .collect())
}

fn key_refs(keys: &[(String, String)]) -> Vec<(&str, &str)> {
    keys.iter()
        .map(|(id, key)| (id.as_str(), key.as_str()))
        .collect()
}

/// Fetch (or read) a package and install it under `~/.perry` after every
/// check passes: the one install path behind `perry target add` and the
/// compile-time ext fetch.
pub(crate) fn install_package(
    kind: PackageKind,
    triple: &str,
    me: PerryIdentity<'_>,
    home: &Path,
    source: Source,
    trusted_keys: &[(&str, &str)],
) -> Result<PathBuf> {
    let version_dir = perry_home_kind(home, kind).join(me.version);
    fs::create_dir_all(&version_dir)
        .with_context(|| format!("cannot create {}", version_dir.display()))?;
    let download = tempfile::Builder::new()
        .prefix(".download-")
        .tempdir_in(&version_dir)?;
    let (tarball, manifest) = match source {
        Source::Files { tarball, manifest } => (
            tarball,
            fs::read(&manifest).with_context(|| format!("cannot read {}", manifest.display()))?,
        ),
        Source::Release => download_release_asset(kind, triple, me.version, download.path())?,
    };
    install_verified(
        &tarball,
        &manifest,
        kind,
        triple,
        me,
        trusted_keys,
        &version_dir,
    )
}

/// What [`ensure_package`] did.
pub(crate) enum PackageFetch {
    /// Downloaded, verified and installed under ~/.perry/<ext|targets>.
    Installed,
    /// Not attempted or failed; the message says why and what to install.
    Refused(String),
}

/// A compile needs a library of the `kind` package for `triple` (the ext,
/// UI or wasm_host archives of a host, or a cross target's libraries) and no
/// install layout has it. When perry publishes that package and
/// `PERRY_NO_AUTO_FETCH` is unset, install it now, exactly as `perry target
/// add` would, saying so in one line. Only this path touches the network
/// during a compile, and only when such a library is missing.
pub(crate) fn ensure_package(kind: PackageKind, triple: &str) -> Option<PackageFetch> {
    let exe = std::env::current_exe().ok();
    let home = dirs::home_dir();
    fetch_package(
        kind,
        triple,
        PerryIdentity::current(),
        exe.as_deref(),
        home.as_deref(),
        compiled_keys,
    )
}

/// [`ensure_package`] with this perry's identity, paths and keyring
/// injected (the tests sign with their own key).
fn fetch_package(
    kind: PackageKind,
    triple: &str,
    me: PerryIdentity<'_>,
    exe: Option<&Path>,
    home: Option<&Path>,
    keys: impl FnOnce() -> Result<Vec<(String, String)>>,
) -> Option<PackageFetch> {
    if !is_packaged(kind, triple) {
        return None;
    }
    let package = format!("{}@{}", package_name(kind, triple), me.version);
    let hint = || install_hint(kind, triple, &add_command(kind, triple), exe, home, me);
    let refused = |why: String| {
        Some(PackageFetch::Refused(format!(
            "it ships in {package}, {why}\n{}",
            hint()
        )))
    };
    if std::env::var_os(NO_AUTO_FETCH_ENV).is_some_and(|v| !v.is_empty() && v != "0") {
        return refused(format!(
            "which is not installed; {NO_AUTO_FETCH_ENV} is set, so perry does not download it."
        ));
    }
    let Some(home) = home else {
        return refused(
            "which is not installed, and there is no home directory to put it in.".into(),
        );
    };
    let keys = match keys() {
        Ok(keys) => keys,
        Err(e) => {
            return refused(format!(
                "which is not installed, and this perry cannot verify a download: {e:#}"
            ))
        }
    };
    let what = match kind {
        PackageKind::Ext => "native extensions".to_string(),
        PackageKind::Ui => "UI libraries".to_string(),
        PackageKind::WasmHost => "WebAssembly host".to_string(),
        PackageKind::Target => format!("libraries for the {triple} target"),
    };
    eprintln!(
        "perry: fetching {package} ({what}, once per perry version) into {}; {NO_AUTO_FETCH_ENV}=1 turns this off",
        perry_home_root(home, kind, me.version, triple).display()
    );
    match install_package(kind, triple, me, home, Source::Release, &key_refs(&keys)) {
        Ok(_) => Some(PackageFetch::Installed),
        Err(e) if e.downcast_ref::<Unreachable>().is_some() => {
            let (lead, tail) = unreachable_advice(kind, triple, me, exe);
            Some(PackageFetch::Refused(format!(
                "it ships in {package}, and fetching it failed: {e:#}\n{lead}{}{tail}",
                hint()
            )))
        }
        Err(e) => refused(format!("and fetching it failed: {e:#}")),
    }
}

/// The release host did not answer at all: resolve, connect, TLS or a
/// deadline, with no HTTP status. A 404 is not this (it says the release
/// has no such package).
#[derive(Debug)]
struct Unreachable {
    host: String,
    cause: String,
}

impl std::fmt::Display for Unreachable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot reach {} ({})", self.host, self.cause)
    }
}

impl std::error::Error for Unreachable {}

/// What to offer when the release host is unreachable, as `(lead, tail)`
/// around [`install_hint`]: the pinned npm command first when perry did not
/// come from npm (npm uses the user's own registry, proxy and credentials;
/// an npm-installed perry already gets it first from `install_hint`), then
/// the proxy and mirror ways to keep using the signed GitHub assets.
fn unreachable_advice(
    kind: PackageKind,
    triple: &str,
    me: PerryIdentity<'_>,
    exe: Option<&Path>,
) -> (String, String) {
    let package = format!("{}@{}", package_name(kind, triple), me.version);
    let lead = if exe.and_then(npm_install).is_some() {
        String::new()
    } else {
        format!(
            "Install it with npm, which uses your own registry and proxy:\n  npm install -g {package}\n"
        )
    };
    let asset = asset_name(kind, triple, me.version);
    let tail = format!(
        "If the release host is blocked, set HTTPS_PROXY, or set {RELEASE_MIRROR_ENV}=<dir|https url> holding {asset} and {asset}.update.json\n"
    );
    (lead, tail)
}

/// `perry target add` failing with [`Unreachable`] gets the same advice.
fn with_unreachable_advice(
    error: anyhow::Error,
    kind: PackageKind,
    triple: &str,
    me: PerryIdentity<'_>,
    exe: Option<&Path>,
) -> anyhow::Error {
    if error.downcast_ref::<Unreachable>().is_none() {
        return error;
    }
    let (lead, tail) = unreachable_advice(kind, triple, me, exe);
    anyhow::anyhow!("{error:#}\n{lead}{tail}")
}

/// Fetch the signed manifest and the tarball it names into `dir`, from
/// `PERRY_RELEASE_MIRROR` when set, else this perry's GitHub release.
fn download_release_asset(
    kind: PackageKind,
    triple: &str,
    version: &str,
    dir: &Path,
) -> Result<(PathBuf, Vec<u8>)> {
    let asset = asset_name(kind, triple, version);
    let mirror = std::env::var(RELEASE_MIRROR_ENV)
        .ok()
        .filter(|m| !m.is_empty());
    if let Some(mirror) = mirror.as_deref().filter(|m| !m.contains("://")) {
        // A local mirror: the same files, read instead of downloaded.
        let base = Path::new(mirror);
        let manifest_path = base.join(format!("{asset}.update.json"));
        let manifest = fs::read(&manifest_path).with_context(|| {
            format!(
                "{RELEASE_MIRROR_ENV}={mirror} has no {}",
                manifest_path.display()
            )
        })?;
        return Ok((base.join(&asset), manifest));
    }
    let base = match mirror {
        Some(url) => {
            crate::update_checker::require_https(&url, RELEASE_MIRROR_ENV)?;
            url.trim_end_matches('/').to_string()
        }
        None => format!("{RELEASE_DOWNLOAD_BASE}/{}", release_tag(version)),
    };
    let client = perry_http_client::Client::with_timeout(Duration::from_secs(600))
        .user_agent(format!("perry/{version}"));
    let manifest_url = format!("{base}/{asset}.update.json");
    // No status at all means the host was not reached; a status (404
    // included) means it answered.
    let response = client
        .execute(perry_http_client::Request::get(&manifest_url))
        .map_err(|e| {
            anyhow::Error::new(Unreachable {
                host: host_of(&manifest_url).to_string(),
                cause: e.to_string(),
            })
        })?;
    if response.status == 404 {
        bail!(
            "{base} has no {asset}: perry {version} has no {} package",
            package_name(kind, triple)
        );
    }
    let manifest = response
        .error_for_status()
        .with_context(|| format!("cannot download {manifest_url}"))?
        .body;
    // The tarball URL comes from the manifest (from the mirror, its file
    // name), which install_verified checks against the signature before
    // anything is unpacked.
    let parsed: perry_update_manifest::TargetPackageManifest =
        serde_json::from_slice(&manifest).context("package manifest is malformed")?;
    if parsed.artifact.name != asset {
        bail!("{manifest_url} names {}, not {asset}", parsed.artifact.name);
    }
    let url = if base.starts_with(RELEASE_DOWNLOAD_BASE) {
        parsed.artifact.url.clone()
    } else {
        format!("{base}/{asset}")
    };
    crate::update_checker::require_https(&url, "package")?;
    let tarball = dir.join(&asset);
    let size = parsed.artifact.size;
    eprintln!(
        "perry: downloading {asset} ({}) from {}",
        megabytes(size),
        host_of(&url)
    );
    let mut sink = FileSink {
        file: fs::File::create(&tarball)?,
        progress: Progress::new(size, std::io::stderr().is_terminal(), Instant::now()),
    };
    // The client's timeout is a whole-request budget: size it to the file.
    client
        .timeout(tarball_timeout(size))
        .execute_streaming(perry_http_client::Request::get(&url), &mut sink)
        .with_context(|| format!("cannot download {url}"))?
        .error_for_status()
        .with_context(|| format!("cannot download {url}"))?;
    sink.file.sync_all()?;
    eprint!("{}", sink.progress.finish(Instant::now()));
    Ok((tarball, manifest))
}

/// The slowest link a tarball download is still given time for.
const MIN_DOWNLOAD_RATE: u64 = 32 * 1024;
/// Every request gets at least this long.
const MIN_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// The whole-request budget for a tarball of `size` bytes: the flat
/// minimum, or the time [`MIN_DOWNLOAD_RATE`] needs when that is longer.
fn tarball_timeout(size: u64) -> Duration {
    MIN_REQUEST_TIMEOUT.max(Duration::from_secs(size / MIN_DOWNLOAD_RATE))
}

/// `31.2 MB` (decimal, as GitHub and npm show sizes).
fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// The host of an `https://host/...` URL.
fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

/// Download progress: a redrawn line on a terminal (at most every 250 ms),
/// one summary line otherwise. A pure state machine over the clock it is
/// given, so the output is testable.
struct Progress {
    total: u64,
    done: u64,
    tty: bool,
    started: Instant,
    last_draw: Option<Instant>,
}

impl Progress {
    fn new(total: u64, tty: bool, now: Instant) -> Self {
        Self {
            total,
            done: 0,
            tty,
            started: now,
            last_draw: None,
        }
    }

    /// Count `bytes` more; the text to print now, if any.
    fn advance(&mut self, bytes: u64, now: Instant) -> Option<String> {
        self.done += bytes;
        if !self.tty
            || self
                .last_draw
                .is_some_and(|t| now.duration_since(t) < Duration::from_millis(250))
        {
            return None;
        }
        self.last_draw = Some(now);
        let percent = if self.total == 0 {
            0
        } else {
            (self.done.min(self.total) * 100 / self.total) as u32
        };
        Some(format!(
            "\r  {} / {}  {percent}%",
            megabytes(self.done).trim_end_matches(" MB"),
            megabytes(self.total)
        ))
    }

    /// The closing line: what arrived and how long it took.
    fn finish(&self, now: Instant) -> String {
        let line = format!(
            "done: {} in {} s\n",
            megabytes(self.done),
            now.duration_since(self.started).as_secs()
        );
        if self.tty {
            format!("\r\x1b[K{line}")
        } else {
            line
        }
    }
}

struct FileSink {
    file: fs::File,
    progress: Progress,
}

impl perry_http_client::BodySink for FileSink {
    fn on_head(&mut self, _status: u16, _len: Option<u64>) -> perry_http_client::Result<()> {
        Ok(())
    }
    fn on_chunk(&mut self, bytes: &[u8]) -> perry_http_client::Result<()> {
        use std::io::Write as _;
        self.file
            .write_all(bytes)
            .map_err(|e| perry_http_client::Error::sink(format!("cannot stage download: {e}")))?;
        if let Some(text) = self.progress.advance(bytes.len() as u64, Instant::now()) {
            eprint!("{text}");
        }
        Ok(())
    }
}

/// Verify `tarball` against the signed `manifest` and install it as
/// `<version_dir>/<triple>`. Every check runs on a private staging copy; the
/// final rename is the only step that touches the destination, so a failed
/// check leaves nothing installed. When another perry installed the same
/// package meanwhile (two compiles fetching at once), its root is kept.
pub(crate) fn install_verified(
    tarball: &Path,
    manifest: &[u8],
    kind: PackageKind,
    triple: &str,
    me: PerryIdentity<'_>,
    trusted_keys: &[(&str, &str)],
    version_dir: &Path,
) -> Result<PathBuf> {
    let manifest: perry_update_manifest::TargetPackageManifest =
        serde_json::from_slice(manifest).context("package manifest is malformed")?;
    perry_update_manifest::verify_target_manifest(&manifest, triple, me.version, trusted_keys)
        .context("refusing unauthenticated package manifest")?;
    // The signed asset name binds the kind: an ext tarball never installs
    // as a target package or the reverse.
    let expected = asset_name(kind, triple, me.version);
    if manifest.artifact.name != expected {
        bail!(
            "authenticated manifest names {}, not {expected}",
            manifest.artifact.name
        );
    }
    perry_update_manifest::verify_cli_artifact(tarball, &manifest.artifact)
        .context("refusing package tarball")?;

    fs::create_dir_all(version_dir)?;
    let staging = tempfile::Builder::new()
        .prefix(&format!(".{triple}-"))
        .tempdir_in(version_dir)?;
    let unpacked = staging.path().join("package");
    extract_package(tarball, &unpacked).context("cannot unpack the package")?;
    let target_json = check_root(&unpacked, triple, me)
        .map_err(|rejected| anyhow::anyhow!("refusing package: {}", rejected.reason))?;
    verify_files(&unpacked, &target_json).context("refusing package")?;

    let dest = version_dir.join(triple);
    let usable = |dir: &Path| dir.is_dir() && check_root(dir, triple, me).is_ok();
    if usable(&dest) {
        return Ok(dest);
    }
    if dest.exists() {
        fs::remove_dir_all(&dest).with_context(|| format!("cannot replace {}", dest.display()))?;
    }
    if let Err(e) = fs::rename(&unpacked, &dest) {
        if usable(&dest) {
            return Ok(dest);
        }
        return Err(e).with_context(|| format!("cannot install {}", dest.display()));
    }
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

/// Every package root reachable from this perry, whatever its kind and triple.
fn all_roots(exe: Option<&Path>, home: &Path) -> Vec<(PackageKind, String, PathBuf, RootSource)> {
    let mut out = Vec::new();
    if let Some(exe) = exe {
        let exe = target_packages::real_exe(exe);
        for ancestor in exe.parent().into_iter().flat_map(Path::ancestors) {
            let scope = ancestor.join("node_modules").join(NPM_SCOPE);
            for (name, path) in dir_entries(&scope) {
                for kind in PackageKind::ALL {
                    if let Some(triple) = name.strip_prefix(kind.prefix()) {
                        out.push((kind, triple.to_string(), path.clone(), RootSource::Npm));
                    }
                }
            }
        }
    }
    for kind in PackageKind::ALL {
        for (_, version) in dir_entries(&perry_home_kind(home, kind)) {
            for (triple, path) in dir_entries(&version) {
                if !triple.starts_with('.') {
                    out.push((kind, triple, path, RootSource::PerryHome));
                }
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
        println!(
            "No target or ext packages installed for perry {}.",
            me.version
        );
        return Ok(());
    }
    for (kind, triple, path, source) in roots {
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
            "{}  {version}  {source}  {status}  {}",
            package_name(kind, &triple),
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
    let (kind, triple) = if args.ui || args.wasm_host {
        (
            if args.ui {
                PackageKind::Ui
            } else {
                PackageKind::WasmHost
            },
            args.target
                .as_deref()
                .map(resolve_triple)
                .transpose()?
                .unwrap_or_else(|| HOST_TRIPLE.to_string()),
        )
    } else {
        resolve_package(args.target.as_deref(), args.ext)?
    };
    let name = package_name(kind, &triple);
    let mut removed = 0;
    let doomed: Vec<PathBuf> = if args.all_versions {
        dir_entries(&perry_home_kind(home, kind))
            .into_iter()
            .map(|(_, version)| version.join(&triple))
            .collect()
    } else {
        vec![perry_home_root(home, kind, me.version, &triple)]
    };
    for dir in doomed {
        if dir.is_dir() {
            fs::remove_dir_all(&dir).with_context(|| format!("cannot remove {}", dir.display()))?;
            println!("Removed {}", dir.display());
            removed += 1;
        }
    }
    // npm owns node_modules: name its command, change nothing.
    for (root_kind, root_triple, path, source) in all_roots(exe, home) {
        if source != RootSource::Npm || root_kind != kind || root_triple != triple {
            continue;
        }
        let command = match exe.and_then(npm_install) {
            Some(NpmInstall::Project(dir)) => {
                format!("npm uninstall {name}   (in {})", dir.display())
            }
            _ => format!("npm uninstall -g {name}"),
        };
        println!(
            "{name} is installed by npm at {}; remove it with:\n  {command}",
            path.display()
        );
        removed += 1;
    }
    if removed == 0 {
        println!("No {name} is installed.");
    }
    Ok(())
}

// The fixtures sign with ed25519-dalek, a direct dependency only with updater-cli.
// Beside this file, not in `target/`: the repo's .gitignore claims `target/`.
#[cfg(all(test, feature = "updater-cli"))]
#[path = "target_tests.rs"]
mod target_tests;
