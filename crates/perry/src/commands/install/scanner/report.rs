//! Scan report: serializable record of what was scanned and what was
//! found. Written to `.perry/install-report.json` after every run.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Block the install unless explicitly overridden.
    P0,
    /// Warn only — printed but not enforced.
    P1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Clean,
    Blocked,
    Overridden,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// `name@version` of the offending package.
    pub package: String,
    /// Absolute path on disk.
    pub package_path: String,
    pub severity: Severity,
    /// Stable identifier — useful for `--allow-risky-rule <id>` later.
    pub rule: String,
    pub message: String,
    /// File path (+ optional `:line`) where the signal was observed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Set when the user bypassed this P0 via `--allow-risky[-all]`.
    /// Always false for P1.
    #[serde(default)]
    pub overridden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub scanned_at: String,
    pub package_count: usize,
    pub findings: Vec<Finding>,
    pub verdict: Verdict,
}

impl ScanReport {
    pub fn write_to(&self, project_root: &Path) -> Result<()> {
        let dir = project_root.join(".perry");
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let path = dir.join("install-report.json");
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn p0_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| matches!(f.severity, Severity::P0))
            .count()
    }

    pub fn p1_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| matches!(f.severity, Severity::P1))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn writes_pretty_json() {
        let td = TempDir::new().unwrap();
        let report = ScanReport {
            scanned_at: "2026-01-01T00:00:00Z".into(),
            package_count: 0,
            findings: vec![],
            verdict: Verdict::Clean,
        };
        report.write_to(td.path()).unwrap();
        let content = fs::read_to_string(td.path().join(".perry/install-report.json")).unwrap();
        assert!(content.contains("\"verdict\": \"clean\""));
    }
}
