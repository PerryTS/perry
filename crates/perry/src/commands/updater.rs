//! `perry updater` subcommands — sign-side tooling for `@perry/updater`.
//!
//! Closes the loop on issue #229 (v0.5.391 shipped the verify-side
//! `perry_updater_verify_signature_v2`; this is the matching sign-side
//! CLI). Three subcommands:
//!
//! - `keygen`        — generate a fresh Ed25519 keypair (random 32-byte seed).
//! - `sign`          — produce a v2 signature `Ed25519(sha256(binary) || version_utf8)`.
//! - `verify`        — verify a v2 signature against a binary + version (sanity check before publish).
//!
//! Output format: every command emits a JSON object on stdout with
//! base64-encoded byte strings, ready to slot into a manifest with `jq`.
//!
//! Trust model: see docs/src/updater/overview.md. Briefly — the secret
//! key never leaves the release-signing machine; the public key gets
//! baked into your app at build time via `bundledPublicKey` on
//! `UpdaterOptions`.
//!
//! v1 (digest-only) signing is intentionally not exposed here. Existing
//! v1 manifests stay valid for verify-side compatibility, but new
//! deployments should use v2 from day one — the verify-side at
//! `crates/perry-updater/src/core.rs::perry_updater_verify_signature_v2`
//! rejects empty version strings up-front to keep the v1/v2 payload
//! spaces fully disjoint, so a v2-aware client can't be tricked into
//! accepting a v1 signature even when an attacker controls the manifest.

use anyhow::{bail, Context, Result};
use base64::Engine as _;
use clap::Subcommand;
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey, SECRET_KEY_LENGTH};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, clap::Args)]
pub struct UpdaterArgs {
    #[command(subcommand)]
    pub command: UpdaterCommand,
}

#[derive(Debug, Subcommand)]
pub enum UpdaterCommand {
    /// Generate a fresh Ed25519 keypair for signing release manifests.
    ///
    /// Output: JSON with base64-encoded `public_key` and `secret_key`.
    /// Save the secret key to a file with mode 0600 (or equivalent) and
    /// never commit it. The public key gets baked into your app via
    /// `UpdaterOptions.bundledPublicKey`.
    Keygen(KeygenArgs),

    /// Sign a binary for a v2 manifest entry.
    ///
    /// Computes `Ed25519(sha256(binary) || version_utf8)` and emits a JSON
    /// object containing the manifest fields you need: `sha256`,
    /// `signature`, `size`. Pipe through `jq` to compose the final
    /// manifest.
    Sign(SignArgs),

    /// Verify a v2 signature against a binary + version (sanity check).
    ///
    /// Same logic the runtime `perry_updater_verify_signature_v2` runs.
    /// Use this in CI before uploading the manifest to catch
    /// version-mismatch / wrong-key / corrupted-binary issues early.
    Verify(VerifyArgs),

    /// Sign the authenticated manifest consumed by `perry update`.
    SignCliManifest(SignCliManifestArgs),
}

#[derive(Debug, clap::Args)]
pub struct KeygenArgs {
    /// Optional path to write the keypair JSON. If omitted, prints to stdout.
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
pub struct SignArgs {
    /// Path to the binary to sign.
    #[arg(short, long)]
    pub binary: PathBuf,

    /// Version string to bind into the signature. MUST match the
    /// `version` field of the manifest entry that will reference this
    /// signature.
    #[arg(short, long)]
    pub version: String,

    /// Path to the secret-key file. Reads the JSON keypair produced by
    /// `keygen` and uses the `secret_key` field. As an alternative,
    /// pass a base64-encoded 32-byte seed via `--secret-key-b64`.
    #[arg(short = 'k', long)]
    pub secret_key: Option<PathBuf>,

    /// Inline base64-encoded 32-byte secret key seed (alternative to
    /// `--secret-key`). Useful for CI where the secret comes from a
    /// repository secret.
    #[arg(long, conflicts_with = "secret_key")]
    pub secret_key_b64: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct SignCliManifestArgs {
    /// Release archive to hash and describe.
    #[arg(long)]
    pub artifact: PathBuf,
    /// Exact release artifact filename (for example `perry-linux-x86_64.tar.gz`).
    #[arg(long)]
    pub platform: String,
    /// HTTPS URL from which the artifact will be downloaded.
    #[arg(long)]
    pub url: String,
    /// Version bound into the signed manifest.
    #[arg(long = "release-version")]
    pub release_version: String,
    /// Stable identifier for the trusted public key; used for key rotation.
    #[arg(long)]
    pub key_id: String,
    /// Base64-encoded 32-byte secret-key seed from protected release CI.
    #[arg(long)]
    pub secret_key_b64: String,
}

#[derive(Debug, clap::Args)]
pub struct VerifyArgs {
    /// Path to the binary to verify.
    #[arg(short, long)]
    pub binary: PathBuf,

    /// Version string the signature was produced over.
    #[arg(short, long)]
    pub version: String,

    /// Base64-encoded 64-byte Ed25519 signature.
    #[arg(short, long)]
    pub signature: String,

    /// Base64-encoded 32-byte Ed25519 public key.
    #[arg(short = 'k', long)]
    pub pubkey: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct KeypairJson {
    public_key: String,
    secret_key: String,
}

/// Public command-dispatch entrypoint.
pub fn run(args: UpdaterArgs) -> Result<()> {
    match args.command {
        UpdaterCommand::Keygen(a) => run_keygen(a),
        UpdaterCommand::Sign(a) => run_sign(a),
        UpdaterCommand::Verify(a) => run_verify(a),
        UpdaterCommand::SignCliManifest(a) => run_sign_cli_manifest(a),
    }
}

fn run_keygen(args: KeygenArgs) -> Result<()> {
    use rand::TryRngCore;
    let mut seed = [0u8; SECRET_KEY_LENGTH];
    rand::rngs::OsRng
        .try_fill_bytes(&mut seed)
        .context("failed to read 32 random bytes from the OS RNG")?;
    let signing = SigningKey::from_bytes(&seed);
    let verifying = signing.verifying_key();

    let kp = KeypairJson {
        public_key: base64::engine::general_purpose::STANDARD.encode(verifying.to_bytes()),
        secret_key: base64::engine::general_purpose::STANDARD.encode(signing.to_bytes()),
    };
    let json = serde_json::to_string_pretty(&kp)?;

    match args.output {
        Some(path) => {
            std::fs::write(&path, &json)
                .with_context(|| format!("failed to write {}", path.display()))?;
            // Best-effort 0600 on Unix so the secret isn't world-readable.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            eprintln!("wrote keypair to {}", path.display());
        }
        None => {
            println!("{}", json);
            eprintln!();
            eprintln!(
                "WARNING: secret key emitted on stdout. Re-run with --output \
                 <path> to write to a file with 0600 permissions."
            );
        }
    }
    Ok(())
}

fn run_sign(args: SignArgs) -> Result<()> {
    if args.version.is_empty() {
        bail!(
            "version must be non-empty (an empty v2 payload would reduce \
             to a v1 signature byte-for-byte; the verify-side rejects \
             this case at runtime — see crates/perry-updater/src/core.rs)"
        );
    }

    let secret_b64 = match (&args.secret_key, &args.secret_key_b64) {
        (Some(path), None) => {
            let raw = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let kp: KeypairJson = serde_json::from_str(&raw).with_context(|| {
                format!(
                    "{} is not a valid keypair JSON (run `perry updater keygen` to produce one)",
                    path.display()
                )
            })?;
            kp.secret_key
        }
        (None, Some(b64)) => b64.clone(),
        (None, None) => {
            bail!("must pass either --secret-key <file> or --secret-key-b64 <inline-base64>");
        }
        (Some(_), Some(_)) => unreachable!("clap conflicts_with"),
    };

    let secret_bytes = base64::engine::general_purpose::STANDARD
        .decode(secret_b64.trim())
        .context("secret_key is not valid base64")?;
    if secret_bytes.len() != SECRET_KEY_LENGTH {
        bail!(
            "secret key must decode to {} bytes (got {})",
            SECRET_KEY_LENGTH,
            secret_bytes.len()
        );
    }
    let mut seed = [0u8; SECRET_KEY_LENGTH];
    seed.copy_from_slice(&secret_bytes);
    let signing = SigningKey::from_bytes(&seed);

    let (digest, size) = sha256_file(&args.binary)?;

    // payload = digest_bytes (32) || version_utf8
    let mut payload = Vec::with_capacity(32 + args.version.len());
    payload.extend_from_slice(&digest);
    payload.extend_from_slice(args.version.as_bytes());

    let signature = signing.sign(&payload);

    let envelope = serde_json::json!({
        "schemaVersion": 2,
        "version": args.version,
        "size": size,
        "sha256": hex::encode(digest),
        "signature": base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
        "publicKey": base64::engine::general_purpose::STANDARD.encode(signing.verifying_key().to_bytes()),
    });
    println!("{}", serde_json::to_string_pretty(&envelope)?);
    Ok(())
}

fn run_sign_cli_manifest(args: SignCliManifestArgs) -> Result<()> {
    if args.release_version.is_empty() || args.key_id.is_empty() || args.platform.is_empty() {
        bail!("version, key-id, and platform must be non-empty");
    }
    let parsed = url::Url::parse(&args.url).context("invalid artifact URL")?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || parsed.username() != ""
        || parsed.password().is_some()
    {
        bail!("artifact URL must be an absolute HTTPS URL without credentials");
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(args.secret_key_b64.trim())?;
    let seed: [u8; SECRET_KEY_LENGTH] = bytes.try_into().map_err(|v: Vec<u8>| {
        anyhow::anyhow!(
            "secret key must decode to {} bytes (got {})",
            SECRET_KEY_LENGTH,
            v.len()
        )
    })?;
    let signing = SigningKey::from_bytes(&seed);
    let (digest, size) = sha256_file(&args.artifact)?;
    let name = args
        .artifact
        .file_name()
        .and_then(|v| v.to_str())
        .context("artifact filename is not UTF-8")?
        .to_string();
    if name != args.platform {
        bail!("artifact filename must equal --platform to prevent platform confusion");
    }
    let mut manifest = perry_updater::cli_manifest::CliUpdateManifest {
        schema_version: 1,
        key_id: args.key_id,
        version: args.release_version,
        platform: args.platform,
        artifact: perry_updater::cli_manifest::CliUpdateArtifact {
            name,
            url: args.url,
            sha256: hex::encode(digest),
            size,
        },
        signature: String::new(),
    };
    perry_updater::cli_manifest::sign_cli_manifest(&mut manifest, &signing)?;
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}

fn run_verify(args: VerifyArgs) -> Result<()> {
    if args.version.is_empty() {
        bail!("version must be non-empty");
    }

    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(args.signature.trim())
        .context("signature is not valid base64")?;
    if sig_bytes.len() != 64 {
        bail!(
            "signature must decode to 64 bytes (got {})",
            sig_bytes.len()
        );
    }
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);
    let signature = ed25519_dalek::Signature::from_bytes(&sig_arr);

    let pk_bytes = base64::engine::general_purpose::STANDARD
        .decode(args.pubkey.trim())
        .context("pubkey is not valid base64")?;
    if pk_bytes.len() != 32 {
        bail!("pubkey must decode to 32 bytes (got {})", pk_bytes.len());
    }
    let mut pk_arr = [0u8; 32];
    pk_arr.copy_from_slice(&pk_bytes);
    let verifying =
        VerifyingKey::from_bytes(&pk_arr).context("pubkey is not a valid Ed25519 point")?;

    let (digest, _size) = sha256_file(&args.binary)?;
    let mut payload = Vec::with_capacity(32 + args.version.len());
    payload.extend_from_slice(&digest);
    payload.extend_from_slice(args.version.as_bytes());

    use ed25519_dalek::Verifier as _;
    match verifying.verify(&payload, &signature) {
        Ok(()) => {
            println!("OK");
            Ok(())
        }
        Err(e) => bail!("signature verification failed: {}", e),
    }
}

fn sha256_file(path: &std::path::Path) -> Result<([u8; 32], u64)> {
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    Ok((out, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// End-to-end roundtrip: keygen → sign → verify. Catches API drift
    /// in any of the three command implementations and proves the
    /// emitted JSON envelope feeds back into verify cleanly.
    #[test]
    fn keygen_sign_verify_roundtrip() {
        // 1) Keygen — to a tmp file.
        let dir =
            std::env::temp_dir().join(format!("perry-updater-cli-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let kp_path = dir.join("kp.json");
        run_keygen(KeygenArgs {
            output: Some(kp_path.clone()),
        })
        .unwrap();
        let kp_raw = std::fs::read_to_string(&kp_path).unwrap();
        let kp: KeypairJson = serde_json::from_str(&kp_raw).unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(&kp.secret_key)
                .unwrap()
                .len(),
            32
        );

        // 2) Sign — capture stdout via a quick fork: we re-implement the
        //    body of run_sign here without the println! so the
        //    JSON-envelope is callable from a unit test without
        //    capturing stdout. This is exactly the same payload-build
        //    + sign call that run_sign would emit.
        let bin_path = dir.join("payload.bin");
        let mut f = std::fs::File::create(&bin_path).unwrap();
        f.write_all(b"sign-me").unwrap();
        drop(f);

        let secret_bytes = base64::engine::general_purpose::STANDARD
            .decode(&kp.secret_key)
            .unwrap();
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&secret_bytes);
        let signing = SigningKey::from_bytes(&seed);
        let (digest, _size) = sha256_file(&bin_path).unwrap();
        let mut payload = Vec::new();
        payload.extend_from_slice(&digest);
        payload.extend_from_slice(b"1.2.3");
        let signature = signing.sign(&payload);

        // 3) Verify — through the actual run_verify path.
        run_verify(VerifyArgs {
            binary: bin_path.clone(),
            version: "1.2.3".into(),
            signature: base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
            pubkey: kp.public_key.clone(),
        })
        .unwrap();

        // 4) Wrong version fails.
        let bad = run_verify(VerifyArgs {
            binary: bin_path.clone(),
            version: "9.9.9".into(),
            signature: base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
            pubkey: kp.public_key.clone(),
        });
        assert!(bad.is_err());

        // Cleanup.
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Empty version is rejected at sign-side (matches the verify-side
    /// invariant in `perry_updater_verify_signature_v2`).
    #[test]
    fn empty_version_rejected() {
        let dir =
            std::env::temp_dir().join(format!("perry-updater-cli-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin_path = dir.join("payload.bin");
        std::fs::write(&bin_path, b"x").unwrap();

        let r = run_sign(SignArgs {
            binary: bin_path,
            version: String::new(),
            secret_key: None,
            secret_key_b64: Some(base64::engine::general_purpose::STANDARD.encode([7u8; 32])),
        });
        assert!(r.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
