use super::*;
use crate::commands::compile::target_packages::{TargetFile, TargetJson};
use ed25519_dalek::SigningKey;
use perry_base64::Engine as _;
use sha2::{Digest, Sha256};

const TRIPLE: &str = "wasm32-wasip1";
const ME: PerryIdentity<'static> = PerryIdentity {
    version: "0.5.1700",
    revision: Some("1111111111111111111111111111111111111111"),
};
const KEY_ID: &str = "release-2026";
const ARCHIVE: &[u8] = b"!<arch>\nwasm runtime";

fn tree() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    (dir, root)
}

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[9; 32])
}

fn public_key() -> String {
    perry_base64::engine::general_purpose::STANDARD.encode(signing_key().verifying_key().to_bytes())
}

/// The package as stage-npm.sh writes it: archive compressed, sidecar raw.
fn target_json(version: &str, triple: &str) -> TargetJson {
    TargetJson {
        schema_version: 1,
        perry_version: version.into(),
        source_revision: ME.revision.unwrap().into(),
        target_triple: triple.into(),
        files: vec![TargetFile {
            path: "lib/libperry_runtime.a".into(),
            sha256: perry_hex::encode(Sha256::digest(ARCHIVE)),
            size: ARCHIVE.len() as u64,
        }],
    }
}

/// An npm-style tarball (`package/...`) holding `target_json` and `archive`.
fn tarball(dir: &Path, manifest: &TargetJson, archive: &[u8]) -> PathBuf {
    let path = dir.join(asset_name(PackageKind::Target, TRIPLE, ME.version));
    let file = fs::File::create(&path).unwrap();
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        file,
        flate2::Compression::fast(),
    ));
    let mut add = |name: &str, bytes: &[u8]| {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, format!("package/{name}"), bytes)
            .unwrap();
    };
    add("package.json", b"{}");
    add(TARGET_JSON, &serde_json::to_vec(manifest).unwrap());
    add(
        "lib/libperry_runtime.a.zst",
        &zstd::encode_all(archive, 3).unwrap(),
    );
    builder.into_inner().unwrap().finish().unwrap();
    path
}

fn signed_manifest(tgz: &Path) -> perry_update_manifest::TargetPackageManifest {
    let bytes = fs::read(tgz).unwrap();
    let name = asset_name(PackageKind::Target, TRIPLE, ME.version);
    let mut manifest = perry_update_manifest::TargetPackageManifest {
        schema_version: 1,
        key_id: KEY_ID.into(),
        version: ME.version.into(),
        target_triple: TRIPLE.into(),
        artifact: perry_update_manifest::CliUpdateArtifact {
            url: format!("https://example.invalid/{name}"),
            name,
            sha256: perry_hex::encode(Sha256::digest(&bytes)),
            size: bytes.len() as u64,
        },
        signature: String::new(),
    };
    perry_update_manifest::sign_target_manifest(&mut manifest, &signing_key()).unwrap();
    manifest
}

fn install(tgz: &Path, manifest: &[u8], version_dir: &Path) -> Result<PathBuf> {
    let public = public_key();
    install_verified(
        tgz,
        manifest,
        PackageKind::Target,
        TRIPLE,
        ME,
        &[(KEY_ID, public.as_str())],
        version_dir,
    )
}

/// Nothing at all under the version dir: no destination, no staging leftovers.
fn assert_nothing_installed(version_dir: &Path) {
    let left: Vec<_> = fs::read_dir(version_dir)
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "left behind: {left:?}");
}

#[test]
fn a_good_package_installs_atomically_and_resolves() {
    let (_t, root) = tree();
    let tgz = tarball(&root, &target_json(ME.version, TRIPLE), ARCHIVE);
    let manifest = serde_json::to_vec(&signed_manifest(&tgz)).unwrap();
    let home = root.join("home");
    let version_dir = perry_home_kind(&home, PackageKind::Target).join(ME.version);

    let dest = install(&tgz, &manifest, &version_dir).unwrap();
    assert_eq!(
        dest,
        perry_home_root(&home, PackageKind::Target, ME.version, TRIPLE)
    );
    let entries: Vec<_> = fs::read_dir(&version_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(
        entries,
        [std::ffi::OsString::from(TRIPLE)],
        "no staging left"
    );
    let roots = discover(PackageKind::Target, TRIPLE, None, Some(&home), ME);
    assert_eq!(roots.len(), 1);
    assert!(roots[0].is_accepted());
    verify_files(&dest, roots[0].status.as_ref().unwrap()).unwrap();
}

#[test]
fn every_failed_check_leaves_nothing_installed() {
    let (_t, root) = tree();
    let version_dir = root.join("home/.perry/targets").join(ME.version);
    let good_tgz = tarball(&root, &target_json(ME.version, TRIPLE), ARCHIVE);
    let good = signed_manifest(&good_tgz);

    let mut cases: Vec<(&str, PathBuf, Vec<u8>)> = Vec::new();

    let mut bad_sig = good.clone();
    bad_sig.signature = perry_base64::engine::general_purpose::STANDARD.encode([0_u8; 64]);
    cases.push((
        "bad signature",
        good_tgz.clone(),
        serde_json::to_vec(&bad_sig).unwrap(),
    ));

    // A CLI-update manifest for the same bytes, signed with the same key.
    let mut cli = perry_update_manifest::CliUpdateManifest {
        schema_version: 1,
        key_id: KEY_ID.into(),
        version: ME.version.into(),
        platform: good.artifact.name.clone(),
        artifact: good.artifact.clone(),
        signature: String::new(),
    };
    perry_update_manifest::sign_cli_manifest(&mut cli, &signing_key()).unwrap();
    cases.push((
        "CLI manifest",
        good_tgz.clone(),
        serde_json::to_vec(&cli).unwrap(),
    ));

    let mut wrong_sha = good.clone();
    wrong_sha.artifact.sha256 = "00".repeat(32);
    perry_update_manifest::sign_target_manifest(&mut wrong_sha, &signing_key()).unwrap();
    cases.push((
        "sha mismatch",
        good_tgz.clone(),
        serde_json::to_vec(&wrong_sha).unwrap(),
    ));

    let mut wrong_size = good.clone();
    wrong_size.artifact.size += 1;
    perry_update_manifest::sign_target_manifest(&mut wrong_size, &signing_key()).unwrap();
    cases.push((
        "size mismatch",
        good_tgz.clone(),
        serde_json::to_vec(&wrong_size).unwrap(),
    ));

    let mut other_version = good.clone();
    other_version.version = "0.5.1699".into();
    perry_update_manifest::sign_target_manifest(&mut other_version, &signing_key()).unwrap();
    cases.push((
        "manifest version",
        good_tgz.clone(),
        serde_json::to_vec(&other_version).unwrap(),
    ));

    for (label, json) in [
        ("target.json version", target_json("0.5.1699", TRIPLE)),
        (
            "target.json triple",
            target_json(ME.version, "wasm32-wasip2"),
        ),
    ] {
        let dir = root.join(label.replace(' ', "-"));
        fs::create_dir_all(&dir).unwrap();
        let tgz = tarball(&dir, &json, ARCHIVE);
        let manifest = serde_json::to_vec(&signed_manifest(&tgz)).unwrap();
        cases.push((label, tgz, manifest));
    }

    let dir = root.join("per-file");
    fs::create_dir_all(&dir).unwrap();
    let tgz = tarball(&dir, &target_json(ME.version, TRIPLE), b"!<arch>\ntampered");
    let manifest = serde_json::to_vec(&signed_manifest(&tgz)).unwrap();
    cases.push(("per-file hash", tgz, manifest));

    for (label, tgz, manifest) in cases {
        let err = install(&tgz, &manifest, &version_dir).expect_err(label);
        assert_nothing_installed(&version_dir);
        eprintln!("{label}: {err:#}");
    }
}

#[test]
fn remove_deletes_perry_home_and_never_touches_node_modules() {
    let (_t, root) = tree();
    let home = root.join("home");
    let modules = root.join("prefix/lib/node_modules");
    let exe = modules.join("@perryts/perry-linux-x64/bin/perry");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, b"").unwrap();
    let npm = modules.join(package_name(PackageKind::Target, TRIPLE));
    fs::create_dir_all(npm.join("lib")).unwrap();
    fs::write(
        npm.join(TARGET_JSON),
        serde_json::to_vec(&target_json(ME.version, TRIPLE)).unwrap(),
    )
    .unwrap();
    let mine = perry_home_root(&home, PackageKind::Target, ME.version, TRIPLE);
    let older = perry_home_root(&home, PackageKind::Target, "0.5.1600", TRIPLE);
    fs::create_dir_all(&mine).unwrap();
    fs::create_dir_all(&older).unwrap();

    let remove = |all_versions| {
        run_remove(
            RemoveArgs {
                target: Some("wasip1".into()),
                ext: false,
                ui: false,
                wasm_host: false,
                all_versions,
            },
            Some(&exe),
            &home,
            ME,
        )
        .unwrap()
    };
    remove(false);
    assert!(!mine.exists());
    assert!(older.exists(), "only this version without --all-versions");
    assert!(npm.join(TARGET_JSON).exists(), "node_modules is npm's");
    remove(true);
    assert!(!older.exists());
    assert!(npm.join(TARGET_JSON).exists(), "node_modules is npm's");
}

#[test]
fn add_refuses_the_host_triple_and_unknown_names() {
    let (_t, root) = tree();
    let add = |target: &str| {
        run_add(
            AddArgs {
                target: Some(target.into()),
                ext: false,
                ui: false,
                wasm_host: false,
                from: None,
                manifest: None,
            },
            None,
            &root,
            ME,
        )
    };
    let err = add(HOST_TRIPLE).unwrap_err().to_string();
    assert!(err.contains("ship with perry"), "{err}");
    let err = add("Not A Target").unwrap_err().to_string();
    assert!(err.contains("unknown target"), "{err}");
    assert!(!root.join(".perry").exists(), "nothing was created");
}

#[test]
fn target_names_and_triples_resolve_without_a_list() {
    assert_eq!(resolve_triple("android").unwrap(), "aarch64-linux-android");
    assert_eq!(
        resolve_triple("ios-simulator").unwrap(),
        "aarch64-apple-ios-sim"
    );
    assert_eq!(resolve_triple("wasi").unwrap(), "wasm32-wasip2");
    assert_eq!(resolve_triple("wasm32-wasip1").unwrap(), "wasm32-wasip1");
}

// --- ext packages and the compile-time fetch ---------------------------------

const EXT_TRIPLE: &str = "x86_64-unknown-linux-gnu";
const EXT_ARCHIVE: &[u8] = b"!<arch>\nhttp wrapper";

/// An ext package tarball as stage-npm.sh writes it.
fn ext_tarball(dir: &Path, version: &str) -> PathBuf {
    let path = dir.join(asset_name(PackageKind::Ext, EXT_TRIPLE, ME.version));
    let manifest = TargetJson {
        schema_version: 1,
        perry_version: version.into(),
        source_revision: ME.revision.unwrap().into(),
        target_triple: EXT_TRIPLE.into(),
        files: vec![TargetFile {
            path: "lib/libperry_ext_http.a".into(),
            sha256: perry_hex::encode(Sha256::digest(EXT_ARCHIVE)),
            size: EXT_ARCHIVE.len() as u64,
        }],
    };
    let file = fs::File::create(&path).unwrap();
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        file,
        flate2::Compression::fast(),
    ));
    let mut add = |name: &str, bytes: &[u8]| {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, format!("package/{name}"), bytes)
            .unwrap();
    };
    add("package.json", b"{}");
    add(TARGET_JSON, &serde_json::to_vec(&manifest).unwrap());
    add(
        "lib/libperry_ext_http.a.zst",
        &zstd::encode_all(EXT_ARCHIVE, 3).unwrap(),
    );
    builder.into_inner().unwrap().finish().unwrap();
    path
}

/// A local signed test release: `<asset>` and `<asset>.update.json`.
fn ext_release(dir: &Path, version: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let tgz = ext_tarball(dir, version);
    let bytes = fs::read(&tgz).unwrap();
    let name = asset_name(PackageKind::Ext, EXT_TRIPLE, ME.version);
    let mut manifest = perry_update_manifest::TargetPackageManifest {
        schema_version: 1,
        key_id: KEY_ID.into(),
        version: ME.version.into(),
        target_triple: EXT_TRIPLE.into(),
        artifact: perry_update_manifest::CliUpdateArtifact {
            url: format!("https://example.invalid/{name}"),
            name: name.clone(),
            sha256: perry_hex::encode(Sha256::digest(&bytes)),
            size: bytes.len() as u64,
        },
        signature: String::new(),
    };
    perry_update_manifest::sign_target_manifest(&mut manifest, &signing_key()).unwrap();
    fs::write(
        dir.join(format!("{name}.update.json")),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    tgz
}

fn test_keys() -> Result<Vec<(String, String)>> {
    Ok(vec![(KEY_ID.to_string(), public_key())])
}

/// `fetch_package` with the env it reads set for the call only (run
/// with RUST_TEST_THREADS=1 like the other env-touching tests).
fn fetch_kind_with(
    kind: PackageKind,
    mirror: Option<&Path>,
    no_fetch: bool,
    exe: Option<&Path>,
    home: Option<&Path>,
    triple: &str,
) -> Option<PackageFetch> {
    match mirror {
        Some(m) => std::env::set_var(RELEASE_MIRROR_ENV, m),
        None => std::env::remove_var(RELEASE_MIRROR_ENV),
    }
    if no_fetch {
        std::env::set_var(NO_AUTO_FETCH_ENV, "1");
    } else {
        std::env::remove_var(NO_AUTO_FETCH_ENV);
    }
    let out = fetch_package(kind, triple, ME, exe, home, test_keys);
    std::env::remove_var(RELEASE_MIRROR_ENV);
    std::env::remove_var(NO_AUTO_FETCH_ENV);
    out
}

fn fetch_with(
    mirror: Option<&Path>,
    no_fetch: bool,
    exe: Option<&Path>,
    home: Option<&Path>,
    triple: &str,
) -> Option<PackageFetch> {
    fetch_kind_with(PackageKind::Ext, mirror, no_fetch, exe, home, triple)
}

#[test]
fn a_missing_ext_package_is_fetched_from_the_release_and_then_found() {
    let (_t, root) = tree();
    let release = root.join("release");
    ext_release(&release, ME.version);
    let home = root.join("home");
    let Some(PackageFetch::Installed) =
        fetch_with(Some(&release), false, None, Some(&home), EXT_TRIPLE)
    else {
        panic!("the ext package installs from the mirror");
    };
    let dest = perry_home_root(&home, PackageKind::Ext, ME.version, EXT_TRIPLE);
    let roots = discover(PackageKind::Ext, EXT_TRIPLE, None, Some(&home), ME);
    assert_eq!(roots.len(), 1);
    verify_files(&dest, roots[0].status.as_ref().unwrap()).unwrap();
}

#[test]
fn a_concurrent_install_keeps_the_root_already_in_place() {
    let (_t, root) = tree();
    let release = root.join("release");
    let tgz = ext_release(&release, ME.version);
    let manifest = fs::read(release.join(format!(
        "{}.update.json",
        asset_name(PackageKind::Ext, EXT_TRIPLE, ME.version)
    )))
    .unwrap();
    let version_dir = root.join("home/.perry/ext").join(ME.version);
    let public = public_key();
    let keys = [(KEY_ID, public.as_str())];
    let first = install_verified(
        &tgz,
        &manifest,
        PackageKind::Ext,
        EXT_TRIPLE,
        ME,
        &keys,
        &version_dir,
    )
    .unwrap();
    let marker = first.join("lib/marker");
    fs::write(&marker, b"in use").unwrap();
    let second = install_verified(
        &tgz,
        &manifest,
        PackageKind::Ext,
        EXT_TRIPLE,
        ME,
        &keys,
        &version_dir,
    )
    .unwrap();
    assert_eq!(first, second);
    assert!(
        marker.exists(),
        "a usable root is never replaced under a running link"
    );
    let left: Vec<_> = fs::read_dir(&version_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(
        left,
        [std::ffi::OsString::from(EXT_TRIPLE)],
        "no staging left"
    );
}

#[test]
fn the_opt_out_fetches_nothing_and_names_npm_and_perry() {
    let (_t, root) = tree();
    let release = root.join("release");
    ext_release(&release, ME.version);
    let home = root.join("home");
    let modules = root.join("prefix/lib/node_modules");
    let exe = modules.join("@perryts/perry/node_modules/@perryts/perry-linux-x64/bin/perry");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, b"").unwrap();
    let Some(PackageFetch::Refused(message)) =
        fetch_with(Some(&release), true, Some(&exe), Some(&home), EXT_TRIPLE)
    else {
        panic!("PERRY_NO_AUTO_FETCH refuses");
    };
    assert!(message.contains(NO_AUTO_FETCH_ENV), "{message}");
    assert!(
        message.contains("npm install -g @perryts/perry-ext-x86_64-unknown-linux-gnu@0.5.1700"),
        "{message}"
    );
    assert!(message.contains("perry target add --ext"), "{message}");
    assert!(!home.join(".perry").exists(), "nothing was fetched");
}

#[test]
fn a_failed_fetch_installs_nothing_and_says_why() {
    let (_t, root) = tree();
    let home = root.join("home");
    // A release whose tarball does not match its signed manifest.
    let release = root.join("release");
    let tgz = ext_release(&release, ME.version);
    fs::write(&tgz, b"tampered").unwrap();
    let Some(PackageFetch::Refused(message)) =
        fetch_with(Some(&release), false, None, Some(&home), EXT_TRIPLE)
    else {
        panic!("a tampered release is refused");
    };
    assert!(message.contains("fetching it failed"), "{message}");
    assert_nothing_installed(&home.join(".perry/ext").join(ME.version));
    // An empty mirror: no manifest.
    let empty = root.join("empty");
    fs::create_dir_all(&empty).unwrap();
    let Some(PackageFetch::Refused(message)) =
        fetch_with(Some(&empty), false, None, Some(&home), EXT_TRIPLE)
    else {
        panic!("a mirror without the asset is refused");
    };
    assert!(message.contains("has no"), "{message}");
}

#[test]
fn a_triple_without_an_ext_package_is_never_fetched() {
    let (_t, root) = tree();
    let release = root.join("release");
    ext_release(&release, ME.version);
    let home = root.join("home");
    // Every musl and Windows host has an ext package; a cross target (wasm,
    // android, ios) is not a host and never has one.
    for triple in [
        "aarch64-linux-android",
        "wasm32-wasip1",
        "wasm32-wasip2",
        "aarch64-apple-ios",
    ] {
        assert!(
            fetch_with(Some(&release), false, None, Some(&home), triple).is_none(),
            "{triple}"
        );
    }
    assert!(!home.join(".perry").exists());
}

#[test]
fn the_signed_asset_name_binds_the_package_kind() {
    let (_t, root) = tree();
    let release = root.join("release");
    let tgz = ext_release(&release, ME.version);
    let manifest = fs::read(release.join(format!(
        "{}.update.json",
        asset_name(PackageKind::Ext, EXT_TRIPLE, ME.version)
    )))
    .unwrap();
    let version_dir = root.join("home/.perry/targets").join(ME.version);
    let public = public_key();
    let err = install_verified(
        &tgz,
        &manifest,
        PackageKind::Target,
        EXT_TRIPLE,
        ME,
        &[(KEY_ID, public.as_str())],
        &version_dir,
    )
    .unwrap_err();
    assert!(err.to_string().contains("names perry-ext-"), "{err:#}");
    assert_nothing_installed(&version_dir);
}

#[test]
fn add_and_remove_name_the_ext_package_of_this_platform_by_default() {
    assert_eq!(
        resolve_package(None, true).unwrap(),
        (PackageKind::Ext, HOST_TRIPLE.to_string())
    );
    assert_eq!(
        resolve_package(Some("android"), false).unwrap(),
        (PackageKind::Target, "aarch64-linux-android".to_string())
    );
    assert!(resolve_package(None, false).is_err());
    let (_t, root) = tree();
    let err = run_add(
        AddArgs {
            target: Some("windows".into()),
            ext: false,
            ui: false,
            wasm_host: false,
            from: None,
            manifest: None,
        },
        None,
        &root,
        ME,
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("publishes no @perryts/perry-target-x86_64-pc-windows-msvc"),
        "{err}"
    );
    assert!(!root.join(".perry").exists(), "nothing was created");
}

#[test]
fn progress_counts_bytes_and_prints_one_line_off_a_terminal() {
    let t0 = Instant::now();
    let mut progress = Progress::new(31_200_000, false, t0);
    for chunk in [10_000_000, 10_000_000, 11_200_000] {
        assert_eq!(
            progress.advance(chunk, t0),
            None,
            "no redraw off a terminal"
        );
    }
    assert_eq!(progress.done, 31_200_000);
    assert_eq!(
        progress.finish(t0 + Duration::from_secs(14)),
        "done: 31.2 MB in 14 s\n"
    );
}

#[test]
fn progress_redraws_a_terminal_line_at_most_every_250_ms() {
    let t0 = Instant::now();
    let mut progress = Progress::new(31_200_000, true, t0);
    assert_eq!(
        progress.advance(12_400_000, t0).as_deref(),
        Some("\r  12.4 / 31.2 MB  39%")
    );
    let soon = t0 + Duration::from_millis(100);
    assert_eq!(progress.advance(1_000_000, soon), None);
    let later = t0 + Duration::from_millis(300);
    assert_eq!(
        progress.advance(1_000_000, later).as_deref(),
        Some("\r  14.4 / 31.2 MB  46%")
    );
    assert!(progress
        .finish(later)
        .starts_with("\r\x1b[Kdone: 14.4 MB in 0 s"));
    // An unknown size never divides by zero.
    let mut unknown = Progress::new(0, true, t0);
    assert!(unknown.advance(5, t0).unwrap().ends_with("0%"));
}

#[test]
fn the_tarball_timeout_scales_with_size_and_never_drops_below_600_s() {
    assert_eq!(tarball_timeout(0), Duration::from_secs(600));
    assert_eq!(tarball_timeout(10_000_000), Duration::from_secs(600));
    // 62 MB at the 32 KiB/s floor: 1892 s, so a slow link is not cut off
    // after a flat ten minutes.
    assert_eq!(tarball_timeout(62_000_000), Duration::from_secs(1892));
    assert!(tarball_timeout(31_200_000) > Duration::from_secs(600));
}

#[test]
fn the_host_is_named_from_the_url() {
    assert_eq!(
        host_of("https://github.com/PerryTS/perry/x.tgz"),
        "github.com"
    );
    assert_eq!(host_of("https://127.0.0.1:1"), "127.0.0.1:1");
    assert_eq!(host_of("https://mirror.example/a?b"), "mirror.example");
}

#[test]
fn an_unreachable_release_host_is_named_and_the_npm_command_leads() {
    let (_t, root) = tree();
    let home = root.join("home");
    let dead = Path::new("https://127.0.0.1:1");
    // perry from npm: install_hint already leads with the pinned command.
    let modules = root.join("prefix/lib/node_modules");
    let exe = modules.join("@perryts/perry/node_modules/@perryts/perry-linux-x64/bin/perry");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, b"").unwrap();
    let Some(PackageFetch::Refused(message)) =
        fetch_with(Some(dead), false, Some(&exe), Some(&home), EXT_TRIPLE)
    else {
        panic!("an unreachable host is refused");
    };
    let npm = "npm install -g @perryts/perry-ext-x86_64-unknown-linux-gnu@0.5.1700";
    assert!(message.contains("cannot reach 127.0.0.1:1 ("), "{message}");
    let (reach, npm_at, target_add) = (
        message.find("cannot reach").unwrap(),
        message.find(npm).expect(&message),
        message.find("perry target add --ext").unwrap(),
    );
    assert!(reach < npm_at && npm_at < target_add, "{message}");
    assert_eq!(message.matches(npm).count(), 1, "{message}");
    assert!(message.contains("HTTPS_PROXY"), "{message}");
    assert!(
        message.contains("PERRY_RELEASE_MIRROR=<dir|https url> holding perry-ext-"),
        "{message}"
    );
    // perry from anywhere else: the npm command still comes first.
    let Some(PackageFetch::Refused(message)) =
        fetch_with(Some(dead), false, None, Some(&home), EXT_TRIPLE)
    else {
        panic!("an unreachable host is refused");
    };
    assert!(
        message.find(npm).expect(&message) < message.find("perry target add --ext").unwrap(),
        "{message}"
    );
    assert!(!home
        .join(".perry/ext")
        .join(ME.version)
        .join(EXT_TRIPLE)
        .exists());
}

#[test]
fn only_a_missing_status_is_unreachable() {
    let (_t, root) = tree();
    let dir = tempfile::tempdir_in(&root).unwrap();
    std::env::set_var(RELEASE_MIRROR_ENV, "https://127.0.0.1:1");
    let err =
        download_release_asset(PackageKind::Ext, EXT_TRIPLE, ME.version, dir.path()).unwrap_err();
    std::env::remove_var(RELEASE_MIRROR_ENV);
    assert!(err.downcast_ref::<Unreachable>().is_some(), "{err:#}");
    // A mirror directory without the asset is a different failure.
    std::env::set_var(RELEASE_MIRROR_ENV, &root);
    let err =
        download_release_asset(PackageKind::Ext, EXT_TRIPLE, ME.version, dir.path()).unwrap_err();
    std::env::remove_var(RELEASE_MIRROR_ENV);
    assert!(err.downcast_ref::<Unreachable>().is_none(), "{err:#}");
}

// --- target packages and the compile-time fetch -----------------------------

/// A local signed test release of the wasip1 target package.
fn target_release(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let tgz = tarball(dir, &target_json(ME.version, TRIPLE), ARCHIVE);
    let name = asset_name(PackageKind::Target, TRIPLE, ME.version);
    fs::write(
        dir.join(format!("{name}.update.json")),
        serde_json::to_vec(&signed_manifest(&tgz)).unwrap(),
    )
    .unwrap();
    tgz
}

#[test]
fn a_missing_target_package_is_fetched_and_then_found() {
    let (_t, root) = tree();
    let release = root.join("release");
    target_release(&release);
    let home = root.join("home");
    let Some(PackageFetch::Installed) = fetch_kind_with(
        PackageKind::Target,
        Some(&release),
        false,
        None,
        Some(&home),
        TRIPLE,
    ) else {
        panic!("the target package installs from the mirror");
    };
    let dest = perry_home_root(&home, PackageKind::Target, ME.version, TRIPLE);
    assert!(dest.starts_with(home.join(".perry/targets")), "{dest:?}");
    let roots = discover(PackageKind::Target, TRIPLE, None, Some(&home), ME);
    assert_eq!(roots.len(), 1);
    verify_files(&dest, roots[0].status.as_ref().unwrap()).unwrap();
    // The ext package of the same triple is a different kind, still absent.
    assert!(discover(PackageKind::Ext, TRIPLE, None, Some(&home), ME).is_empty());
}

#[test]
fn the_opt_out_refuses_a_target_package_naming_it_and_perry_target_add() {
    let (_t, root) = tree();
    let release = root.join("release");
    target_release(&release);
    let home = root.join("home");
    let Some(PackageFetch::Refused(message)) = fetch_kind_with(
        PackageKind::Target,
        Some(&release),
        true,
        None,
        Some(&home),
        TRIPLE,
    ) else {
        panic!("PERRY_NO_AUTO_FETCH refuses");
    };
    assert!(message.contains(NO_AUTO_FETCH_ENV), "{message}");
    assert!(
        message.contains("@perryts/perry-target-wasm32-wasip1@0.5.1700"),
        "{message}"
    );
    assert!(
        message.contains(&format!("perry target add {TRIPLE}")),
        "{message}"
    );
    assert!(!message.contains("--ext"), "{message}");
    assert!(!home.join(".perry").exists(), "nothing was fetched");
}

#[test]
fn a_signed_target_package_is_refused_as_an_ext_package() {
    let (_t, root) = tree();
    let release = root.join("release");
    let tgz = target_release(&release);
    let manifest = fs::read(release.join(format!(
        "{}.update.json",
        asset_name(PackageKind::Target, TRIPLE, ME.version)
    )))
    .unwrap();
    let version_dir = root.join("home/.perry/ext").join(ME.version);
    let public = public_key();
    let err = install_verified(
        &tgz,
        &manifest,
        PackageKind::Ext,
        TRIPLE,
        ME,
        &[(KEY_ID, public.as_str())],
        &version_dir,
    )
    .unwrap_err();
    assert!(err.to_string().contains("names perry-target-"), "{err:#}");
    assert_nothing_installed(&version_dir);
}

#[test]
fn a_tampered_target_package_installs_nothing() {
    let (_t, root) = tree();
    let release = root.join("release");
    let tgz = target_release(&release);
    fs::write(&tgz, b"tampered").unwrap();
    let home = root.join("home");
    let Some(PackageFetch::Refused(message)) = fetch_kind_with(
        PackageKind::Target,
        Some(&release),
        false,
        None,
        Some(&home),
        TRIPLE,
    ) else {
        panic!("a tampered release is refused");
    };
    assert!(message.contains("fetching it failed"), "{message}");
    assert_nothing_installed(&home.join(".perry/targets").join(ME.version));
}

#[test]
fn the_host_triple_has_no_target_package_to_fetch() {
    let (_t, root) = tree();
    let release = root.join("release");
    target_release(&release);
    let home = root.join("home");
    assert!(fetch_kind_with(
        PackageKind::Target,
        Some(&release),
        false,
        None,
        Some(&home),
        HOST_TRIPLE
    )
    .is_none());
    assert!(!home.join(".perry").exists());
}

#[test]
fn each_host_kind_refusal_names_its_package_and_manual_command() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    for kind in [PackageKind::Ui, PackageKind::WasmHost, PackageKind::Ext] {
        let Some(PackageFetch::Refused(message)) =
            fetch_kind_with(kind, None, true, None, Some(&home), HOST_TRIPLE)
        else {
            panic!("expected refusal")
        };
        assert!(
            message.contains(&format!(
                "{}@{}",
                package_name(kind, HOST_TRIPLE),
                ME.version
            )),
            "{message}"
        );
        assert!(
            message.contains(&format!("perry target add --{}", kind.home_dir())),
            "{message}"
        );
        assert!(message.contains(NO_AUTO_FETCH_ENV), "{message}");
    }
}
