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
    let path = dir.join(asset_name(TRIPLE, ME.version));
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
    let name = asset_name(TRIPLE, ME.version);
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
    let version_dir = perry_home_targets(&home).join(ME.version);

    let dest = install(&tgz, &manifest, &version_dir).unwrap();
    assert_eq!(dest, perry_home_root(&home, ME.version, TRIPLE));
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
    let roots = discover(TRIPLE, None, Some(&home), ME);
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
    let npm = modules.join(package_name(TRIPLE));
    fs::create_dir_all(npm.join("lib")).unwrap();
    fs::write(
        npm.join(TARGET_JSON),
        serde_json::to_vec(&target_json(ME.version, TRIPLE)).unwrap(),
    )
    .unwrap();
    let mine = perry_home_root(&home, ME.version, TRIPLE);
    let older = perry_home_root(&home, "0.5.1600", TRIPLE);
    fs::create_dir_all(&mine).unwrap();
    fs::create_dir_all(&older).unwrap();

    let remove = |all_versions| {
        run_remove(
            RemoveArgs {
                target: "wasip1".into(),
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
                target: target.into(),
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
    assert!(!perry_home_targets(&root).exists(), "nothing was created");
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
