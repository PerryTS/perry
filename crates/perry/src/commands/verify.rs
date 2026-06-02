//! Verify command - submit compiled binary for runtime verification via perry-verify

use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct VerifyArgs {
    /// Path to compiled binary
    pub binary: PathBuf,

    /// Target platform
    #[arg(long, default_value = "linux-x64")]
    pub target: String,

    /// Application type (gui, server, cli)
    #[arg(long, default_value = "server")]
    pub app_type: String,

    /// Auth strategy (none, login-form, api-key, test-mode)
    #[arg(long, default_value = "none")]
    pub auth: String,

    /// Run audit on source before verifying binary
    #[arg(long)]
    pub audit: Option<String>,

    /// Verify service URL
    #[arg(long, default_value = "https://verify.perryts.com")]
    pub verify_url: String,

    /// Poll interval in seconds
    #[arg(long, default_value = "3")]
    pub poll_interval: u64,

    /// Timeout in seconds
    #[arg(long, default_value = "300")]
    pub timeout: u64,

    /// #504 — verify the binary against its sidecar attestation file
    /// (`<binary>.attest.json`) instead of submitting to the remote
    /// runtime-verification service. The attestation is produced by
    /// `perry compile --emit-attest`; this flag recomputes SHA-256
    /// of the binary on disk and reports a single ok/mismatch line.
    /// No network calls.
    #[arg(long)]
    pub attest: bool,
}

// --- Response types matching perry-verify API ---

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct VerifySubmitResponse {
    #[serde(rename = "jobId")]
    pub job_id: String,
    pub status: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct VerifyStatusResponse {
    pub id: Option<String>,
    pub status: String,
    pub steps: Option<Vec<VerifyStep>>,
    pub screenshots: Option<Vec<serde_json::Value>>,
    pub logs: Option<String>,
    #[serde(rename = "durationMs")]
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct VerifyStep {
    pub name: String,
    pub status: String,
    pub method: Option<String>,
    #[serde(rename = "durationMs")]
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
}

/// Core verify logic — reusable from publish.rs
pub async fn run_verify_check(
    binary_path: &PathBuf,
    verify_url: &str,
    target: &str,
    app_type: &str,
    auth: &str,
    poll_interval: u64,
    timeout: u64,
    format: OutputFormat,
) -> Result<VerifyStatusResponse> {
    if !binary_path.exists() {
        bail!("Binary not found: {}", binary_path.display());
    }

    if let OutputFormat::Text = format {
        eprintln!(
            "  {} Submitting binary for verification (target: {})...",
            style("→").cyan(),
            target
        );
    }

    // Read and base64-encode the binary
    let binary_data = fs::read(binary_path)
        .with_context(|| format!("Failed to read {}", binary_path.display()))?;

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&binary_data);

    // Build config and manifest
    let config_json = serde_json::json!({
        "auth": { "strategy": auth }
    })
    .to_string();

    let manifest_json = serde_json::json!({
        "appType": app_type,
        "hasAuthGate": auth != "none",
        "entryScreen": "main"
    })
    .to_string();

    // POST multipart to /verify
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout + 30))
        .build()?;

    let form = reqwest::multipart::Form::new()
        .text("binary_b64", b64)
        .text("target", target.to_string())
        .text("config", config_json)
        .text("manifest", manifest_json);

    let base_url = verify_url.trim_end_matches('/');
    let submit_url = format!("{}/verify", base_url);
    let resp = client
        .post(&submit_url)
        .multipart(form)
        .send()
        .await
        .context("Failed to connect to verify service")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Verify service returned {}: {}", status, body);
    }

    let body = resp.text().await?;
    let submit: VerifySubmitResponse =
        serde_json::from_str(&body).context("Failed to parse verify submit response")?;

    let job_id = &submit.job_id;
    if let OutputFormat::Text = format {
        eprintln!("  {} Job submitted: {}", style("→").cyan(), job_id);
    }

    // Poll for results
    let poll_url = format!("{}/verify/{}", base_url, job_id);
    let start = std::time::Instant::now();
    let timeout_dur = std::time::Duration::from_secs(timeout);
    let poll_dur = std::time::Duration::from_secs(poll_interval);

    loop {
        if start.elapsed() > timeout_dur {
            bail!("Verification timed out after {}s", timeout);
        }

        tokio::time::sleep(poll_dur).await;

        let resp = client
            .get(&poll_url)
            .send()
            .await
            .context("Failed to poll verify status")?;

        if !resp.status().is_success() {
            continue; // Retry on transient errors
        }

        let body = resp.text().await?;
        let status: VerifyStatusResponse =
            serde_json::from_str(&body).context("Failed to parse verify status")?;

        match status.status.as_str() {
            "passed" | "failed" | "error" => {
                if let OutputFormat::Text = format {
                    display_verify_results(&status);
                }
                return Ok(status);
            }
            "running" => {
                // Print step updates
                if let OutputFormat::Text = format {
                    if let Some(ref steps) = status.steps {
                        for step in steps {
                            if step.status == "passed" || step.status == "failed" {
                                let icon = if step.status == "passed" {
                                    style("✓").green()
                                } else {
                                    style("✗").red()
                                };
                                eprintln!(
                                    "    {} {} ({}ms)",
                                    icon,
                                    step.name,
                                    step.duration_ms.unwrap_or(0)
                                );
                            }
                        }
                    }
                }
            }
            _ => {} // pending, etc — keep polling
        }
    }
}

fn display_verify_results(status: &VerifyStatusResponse) {
    eprintln!();
    let (icon, label) = match status.status.as_str() {
        "passed" => (style("✓").green(), style("Verification passed").green()),
        "failed" => (style("✗").red(), style("Verification failed").red()),
        _ => (style("!").yellow(), style("Verification error").yellow()),
    };

    eprintln!("  {} {}", icon, label);

    if let Some(ref steps) = status.steps {
        for step in steps {
            let step_icon = match step.status.as_str() {
                "passed" => style("✓").green(),
                "failed" => style("✗").red(),
                _ => style("·").dim(),
            };
            eprint!(
                "    {} {} ({}ms)",
                step_icon,
                step.name,
                step.duration_ms.unwrap_or(0)
            );
            if let Some(ref err) = step.error {
                if !err.is_empty() {
                    eprint!(" — {}", style(err).red());
                }
            }
            eprintln!();
        }
    }

    if let Some(ref err) = status.error {
        if !err.is_empty() {
            eprintln!("    {}", style(err).red());
        }
    }

    if let Some(ms) = status.duration_ms {
        eprintln!("    Total: {:.1}s", ms as f64 / 1000.0);
    }

    eprintln!();
}

/// Entry point for `perry verify` command
/// #504 — local attestation verification. Reads `<binary>.attest.json`,
/// recomputes SHA-256 of the binary on disk, reports a single ok or
/// mismatch line + exit code. No network, no consent prompt.
fn run_local_attest_verify(binary: &PathBuf, format: OutputFormat) -> Result<()> {
    let canonical = binary.canonicalize().unwrap_or_else(|_| binary.clone());
    let manifest = super::attest::verify_against_sidecar(&canonical)?;
    match format {
        OutputFormat::Text => {
            println!(
                "✓ attestation matches: {}\n  sha256:  {}\n  size:    {} bytes\n  built:   unix-{}\n  perry:   {}\n  commit:  {}",
                canonical.display(),
                manifest.sha256,
                manifest.size,
                manifest.built_at_unix,
                manifest.perry_version,
                if manifest.commit_sha.is_empty() {
                    "<unknown>"
                } else {
                    manifest.commit_sha.as_str()
                },
            );
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "ok": true,
                    "binary": canonical.to_string_lossy(),
                    "manifest": manifest,
                })
            );
        }
    }
    Ok(())
}

pub fn run(args: VerifyArgs, format: OutputFormat, _use_color: bool) -> Result<()> {
    // #504: `--attest` short-circuits to local-attest mode — no
    // tokio runtime, no remote call, no beta-consent prompt.
    // Reads `<binary>.attest.json`, recomputes SHA-256 of the binary
    // on disk, reports a single ok/mismatch line + exit status.
    if args.attest {
        return run_local_attest_verify(&args.binary, format);
    }

    if !crate::commands::publish::check_beta_consent("verify") {
        bail!("Aborted.");
    }

    let target_hint = args.target.clone();

    let rt = tokio::runtime::Runtime::new()?;
    let result = rt.block_on(async {
        // Run audit first if requested
        if let Some(ref audit_path) = args.audit {
            let path = std::path::PathBuf::from(audit_path);
            let path = path.canonicalize().unwrap_or(path);
            super::audit::run_audit_check(
                &path,
                &args.verify_url,
                &args.app_type,
                "all",
                "",
                "D",
                false,
                format,
            )
            .await?;
        }

        let result = run_verify_check(
            &args.binary,
            &args.verify_url,
            &args.target,
            &args.app_type,
            &args.auth,
            args.poll_interval,
            args.timeout,
            format,
        )
        .await;

        match (&result, &format) {
            (Ok(status), OutputFormat::Json) => {
                println!("{}", serde_json::to_string_pretty(status)?);
                if status.status == "failed" || status.status == "error" {
                    std::process::exit(1);
                }
                Ok(())
            }
            (Ok(status), _) => {
                if status.status == "failed" || status.status == "error" {
                    bail!("Verification {}", status.status);
                }
                Ok(())
            }
            (Err(_), OutputFormat::Json) => {
                let err_msg = result.as_ref().unwrap_err().to_string();
                println!(
                    "{}",
                    serde_json::json!({ "error": err_msg, "status": "error" })
                );
                std::process::exit(1);
            }
            (Err(e), _) => Err(anyhow::anyhow!("{}", e)),
        }
    });

    if let Err(ref e) = result {
        crate::commands::publish::report_beta_error(
            "verify",
            &format!("{e:#}"),
            Some(&target_hint),
        );
    }

    result
}
