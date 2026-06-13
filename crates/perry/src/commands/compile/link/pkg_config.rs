//! Validation for user-supplied `pkg-config` package names.
//!
//! `pkg_config` entries come from the user's `package.json`
//! (`perry.nativeLibrary…`) and are passed to `pkg-config --libs <name>`.
//! No shell is involved, so the risk is argument-confusion (e.g. a leading
//! `-` parsed as a flag) rather than shell injection — but validating with a
//! conservative allowlist removes that surface and turns a confusing link
//! failure into an actionable error naming the offending entry (#5068).

use anyhow::{anyhow, Result};

/// Accept a pkg-config package name only if it is a plausible identifier:
/// non-empty, no leading `-` (which `pkg-config` would parse as a flag), and
/// composed solely of characters real package names use.
pub(super) fn validate_pkg_config_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!(
            "invalid pkg-config package name: empty string in perry.nativeLibrary pkg_config"
        ));
    }
    if name.starts_with('-') {
        return Err(anyhow!(
            "invalid pkg-config package name {name:?}: must not start with '-' \
             (would be parsed as a pkg-config flag)"
        ));
    }
    let ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '@' | '+' | '-'));
    if !ok {
        return Err(anyhow!(
            "invalid pkg-config package name {name:?}: only ASCII alphanumerics and \
             the characters . _ / @ + - are allowed"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_real_package_names() {
        for name in ["openssl", "libssl", "gtk+-3.0", "glib-2.0", "sdl2", "zlib"] {
            assert!(
                validate_pkg_config_name(name).is_ok(),
                "{name} should be accepted"
            );
        }
    }

    #[test]
    fn rejects_empty() {
        assert!(validate_pkg_config_name("").is_err());
    }

    #[test]
    fn rejects_leading_dash_flag() {
        assert!(validate_pkg_config_name("--modversion").is_err());
        assert!(validate_pkg_config_name("-lfoo").is_err());
    }

    #[test]
    fn rejects_whitespace_and_metacharacters() {
        for bad in [
            "openssl evil",
            "openssl;rm",
            "open$ssl",
            "open`ssl`",
            "open\nssl",
        ] {
            assert!(
                validate_pkg_config_name(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }
}
