//! Installer detection: pick `bun` if available, else `npm`, else error.

use anyhow::{anyhow, bail, Result};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installer {
    Bun,
    Npm,
}

impl Installer {
    pub fn binary(&self) -> &'static str {
        match self {
            Installer::Bun => "bun",
            Installer::Npm => "npm",
        }
    }

    pub fn print_banner(&self, use_color: bool) {
        let label = match self {
            Installer::Bun => "bun",
            Installer::Npm => "npm",
        };
        if use_color {
            eprintln!(
                "\x1b[1;36mperry install\x1b[0m \x1b[2m(via {} install --ignore-scripts)\x1b[0m",
                label
            );
        } else {
            eprintln!("perry install (via {} install --ignore-scripts)", label);
        }
    }
}

/// Probe whether a binary exists and responds to `--version`.
pub fn probe(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Pick an installer, honoring an optional explicit override.
///
/// Override values are case-insensitive; "bun" or "npm" are accepted.
/// If an override is given and that binary isn't on PATH, we error
/// loudly rather than silently fall back — the user asked for a
/// specific tool.
pub fn pick(override_choice: Option<&str>) -> Result<Installer> {
    if let Some(name) = override_choice {
        let choice = match name.to_ascii_lowercase().as_str() {
            "bun" => Installer::Bun,
            "npm" => Installer::Npm,
            other => bail!(
                "unknown --installer value '{}'; expected 'bun' or 'npm'",
                other
            ),
        };
        if !probe(choice.binary()) {
            return Err(anyhow!(
                "--installer={} requested but `{}` not found on PATH",
                name,
                choice.binary()
            ));
        }
        return Ok(choice);
    }

    if probe("bun") {
        return Ok(Installer::Bun);
    }
    if probe("npm") {
        return Ok(Installer::Npm);
    }
    bail!(
        "neither `bun` nor `npm` is on PATH. Install one of them and re-run, \
         or pass --installer=<name> after putting it on PATH."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_unknown_errors() {
        let err = pick(Some("yarn")).unwrap_err().to_string();
        assert!(err.contains("unknown --installer"));
    }

    #[test]
    fn probe_nonexistent_returns_false() {
        assert!(!probe("definitely-not-a-real-binary-zzz-1234"));
    }
}
