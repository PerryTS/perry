//! Audit command - scan TypeScript source for security vulnerabilities via perry-verify

use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct AuditArgs {
    /// Path to scan (file or directory, default: current directory)
    #[arg(default_value = ".")]
    pub path: String,

    /// Application type hint
    #[arg(long, default_value = "server")]
    pub app_type: String,

    /// Minimum severity to report (all, critical, high)
    #[arg(long, default_value = "all")]
    pub severity: String,

    /// Comma-separated rule IDs to ignore
    #[arg(long, default_value = "")]
    pub ignore: String,

    /// Minimum passing grade (A, B, C, D). Grades below this cause exit code 1
    #[arg(long, default_value = "D")]
    pub fail_on: String,

    /// Enable AI-powered deep scan
    #[arg(long)]
    pub deep_scan: bool,

    /// Verify service URL
    #[arg(long, default_value = "https://verify.perryts.com")]
    pub verify_url: String,
}

// --- Response types matching perry-verify/src/audit/types.ts ---

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AuditResponse {
    pub id: Option<String>,
    pub grade: String,
    pub summary: AuditSummary,
    pub findings: Vec<AuditFinding>,
    #[serde(rename = "durationMs")]
    pub duration_ms: Option<u64>,
    #[serde(rename = "deepScanFindings")]
    pub deep_scan_findings: Option<Vec<AuditFinding>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AuditSummary {
    pub total: u32,
    pub critical: u32,
    pub high: u32,
    pub medium: u32,
    pub low: u32,
    #[serde(rename = "filesScanned")]
    pub files_scanned: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AuditFinding {
    pub rule: String,
    pub severity: String,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub snippet: Option<String>,
    pub fix: Option<String>,
}

/// Collect .ts source files from a directory, skipping common non-source dirs
pub fn collect_source_files(path: &Path) -> Result<HashMap<String, String>> {
    let mut files = HashMap::new();

    if path.is_file() {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        files.insert(name, content);
        return Ok(files);
    }

    for entry in WalkDir::new(path).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !matches!(
            name.as_ref(),
            "node_modules" | ".git" | "dist" | "target" | ".perry" | "build" | ".next"
        )
    }) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let file_path = entry.path();
        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "ts" && ext != "tsx" {
            continue;
        }
        let relative = file_path
            .strip_prefix(path)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();
        match fs::read_to_string(file_path) {
            Ok(content) => {
                files.insert(relative, content);
            }
            Err(e) => {
                eprintln!("  Warning: could not read {}: {}", file_path.display(), e);
            }
        }
    }

    Ok(files)
}

/// Grade ordering for comparison: A > A- > B > C > D > F
fn grade_rank(grade: &str) -> u8 {
    match grade.to_uppercase().as_str() {
        "A" => 6,
        "A-" => 5,
        "B" => 4,
        "C" => 3,
        "D" => 2,
        "F" => 1,
        _ => 0,
    }
}

/// Returns true if the actual grade fails to meet the threshold
pub fn grade_fails_threshold(grade: &str, threshold: &str) -> bool {
    grade_rank(grade) < grade_rank(threshold)
}

/// Core audit logic — reusable from publish.rs
pub async fn run_audit_check(
    project_dir: &Path,
    verify_url: &str,
    app_type: &str,
    severity: &str,
    ignore: &str,
    fail_on: &str,
    deep_scan: bool,
    format: OutputFormat,
) -> Result<AuditResponse> {
    let files = collect_source_files(project_dir)?;
    if files.is_empty() {
        bail!("No .ts files found in {}", project_dir.display());
    }

    if let OutputFormat::Text = format {
        eprintln!(
            "  {} Scanning {} file{}...",
            style("→").cyan(),
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        );
    }

    // Build the source JSON map
    let source_json = serde_json::to_string(&files)?;

    // Build config JSON
    let config = serde_json::json!({
        "appType": app_type,
        "severity": severity,
        "ignore": if ignore.is_empty() {
            vec![]
        } else {
            ignore.split(',').map(|s| s.trim().to_string()).collect::<Vec<_>>()
        },
        "deepScan": deep_scan,
    });
    let config_json = serde_json::to_string(&config)?;

    // POST multipart to /audit
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let form = multipart::Form::new()
        .text("source", source_json)
        .text("config", config_json);

    let url = format!("{}/audit", verify_url.trim_end_matches('/'));
    let resp = client
        .post(&url)
        .multipart(form)
        .send()
        .await
        .context("Failed to connect to audit service")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Audit service returned {}: {}", status, body);
    }

    let body = resp.text().await?;
    let audit: AuditResponse =
        serde_json::from_str(&body).context("Failed to parse audit response")?;

    // Display results
    if let OutputFormat::Text = format {
        display_audit_results(&audit, fail_on);
    }

    // Check threshold
    if grade_fails_threshold(&audit.grade, fail_on) {
        bail!(
            "Audit grade {} does not meet minimum threshold {}. Use --skip-audit to bypass.",
            audit.grade,
            fail_on
        );
    }

    Ok(audit)
}

fn severity_style(severity: &str) -> console::StyledObject<&str> {
    match severity.to_lowercase().as_str() {
        "critical" => style(severity).red().bold(),
        "high" => style(severity).red(),
        "medium" => style(severity).yellow(),
        "low" => style(severity).dim(),
        _ => style(severity),
    }
}

fn grade_style(grade: &str) -> console::StyledObject<&str> {
    match grade {
        "A" | "A-" => style(grade).green().bold(),
        "B" => style(grade).green(),
        "C" => style(grade).yellow(),
        "D" => style(grade).red(),
        "F" => style(grade).red().bold(),
        _ => style(grade),
    }
}

fn display_audit_results(audit: &AuditResponse, fail_on: &str) {
    eprintln!();
    let pass = !grade_fails_threshold(&audit.grade, fail_on);
    let icon = if pass {
        style("✓").green()
    } else {
        style("✗").red()
    };
    eprintln!(
        "  {} Audit grade: {}  ({} finding{})",
        icon,
        grade_style(&audit.grade),
        audit.summary.total,
        if audit.summary.total == 1 { "" } else { "s" }
    );

    if audit.summary.critical > 0 {
        eprintln!(
            "    {} critical: {}",
            style("●").red().bold(),
            audit.summary.critical
        );
    }
    if audit.summary.high > 0 {
        eprintln!("    {} high: {}", style("●").red(), audit.summary.high);
    }
    if audit.summary.medium > 0 {
        eprintln!(
            "    {} medium: {}",
            style("●").yellow(),
            audit.summary.medium
        );
    }
    if audit.summary.low > 0 {
        eprintln!("    {} low: {}", style("●").dim(), audit.summary.low);
    }

    // Show findings
    for finding in &audit.findings {
        eprintln!();
        let location = match (&finding.file, finding.line) {
            (Some(f), Some(l)) => format!("{}:{}", f, l),
            (Some(f), None) => f.clone(),
            _ => String::new(),
        };
        eprintln!(
            "    [{}] {} {}",
            severity_style(&finding.severity),
            finding.message,
            style(&location).dim()
        );
        if let Some(ref snippet) = finding.snippet {
            for line in snippet.lines().take(3) {
                eprintln!("      {}", style(line).dim());
            }
        }
        if let Some(ref fix) = finding.fix {
            eprintln!("      {} {}", style("Fix:").cyan(), fix);
        }
    }

    // Show deep scan findings if present
    if let Some(ref deep) = audit.deep_scan_findings {
        if !deep.is_empty() {
            eprintln!();
            eprintln!("  {} AI deep scan findings:", style("🔍").dim());
            for finding in deep {
                let location = match (&finding.file, finding.line) {
                    (Some(f), Some(l)) => format!("{}:{}", f, l),
                    (Some(f), None) => f.clone(),
                    _ => String::new(),
                };
                eprintln!(
                    "    [{}] {} {}",
                    severity_style(&finding.severity),
                    finding.message,
                    style(&location).dim()
                );
            }
        }
    }

    eprintln!();
}

/// Entry point for `perry audit` command
pub fn run(args: AuditArgs, format: OutputFormat, _use_color: bool) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let path = std::path::PathBuf::from(&args.path);
        let path = path.canonicalize().unwrap_or(path);

        let result = run_audit_check(
            &path,
            &args.verify_url,
            &args.app_type,
            &args.severity,
            &args.ignore,
            &args.fail_on,
            args.deep_scan,
            format,
        )
        .await;

        match (&result, &format) {
            (Ok(audit), OutputFormat::Json) => {
                println!("{}", serde_json::to_string_pretty(audit)?);
                Ok(())
            }
            (Ok(_), _) => Ok(()),
            (Err(_), OutputFormat::Json) => {
                // In JSON mode, output structured error
                let err_msg = result.as_ref().unwrap_err().to_string();
                println!(
                    "{}",
                    serde_json::json!({ "error": err_msg, "grade": serde_json::Value::Null })
                );
                std::process::exit(1);
            }
            (Err(e), _) => Err(anyhow::anyhow!("{}", e)),
        }
    })
}
