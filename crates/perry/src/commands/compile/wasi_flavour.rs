//! The two WASI flavours behind `--target wasi` and `--target wasip1`.
//!
//! One helper owns the CLI name and the Rust/clang triple, so every driver
//! arm asks `Flavour::from_target` instead of matching target literals.
//! [`canonical_target`] folds the spellings other toolchains use into those
//! two names before the driver reads `--target`.

/// Which WASI generation a build targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flavour {
    /// `--target wasi`: a WASI 0.2 component.
    P2,
    /// `--target wasip1`: a WASI preview1 core module.
    P1,
}

impl Flavour {
    /// The flavour a `--target` name selects; `None` for every other target.
    pub(crate) fn from_target(target: &str) -> Option<Self> {
        match target {
            "wasi" => Some(Self::P2),
            "wasip1" => Some(Self::P1),
            _ => None,
        }
    }

    /// The `--target` name.
    #[cfg(feature = "target-wasi")]
    pub(crate) fn target(self) -> &'static str {
        self.cli_name()
    }

    /// The `--target` name, in every build: [`canonical_target`] folds
    /// aliases into it even when the compiler lacks `target-wasi`, so such a
    /// build reports the same "not compiled in" error for every spelling.
    fn cli_name(self) -> &'static str {
        match self {
            Self::P2 => "wasi",
            Self::P1 => "wasip1",
        }
    }

    /// The Rust and clang target triple. The runtime archive is built for it
    /// and found under `target/<triple>/release`, so the flavours never share
    /// an artifact directory.
    pub(crate) fn triple(self) -> &'static str {
        match self {
            Self::P2 => "wasm32-wasip2",
            Self::P1 => "wasm32-wasip1",
        }
    }
}

/// Fold the WASI target spellings other toolchains use into Perry's names
/// (#13953): `wasip2` and the wasi-sdk/Rust triple `wasm32-wasip2` select
/// `wasi`, and `wasm32-wasip1` selects `wasip1`. Without this, those names
/// were unknown targets that fell through to a host link. WASI 0.3 has no
/// command output yet, so its names fail with a pointer to the outputs that
/// exist instead of being linked as a host executable.
pub(crate) fn canonical_target(target: Option<String>) -> anyhow::Result<Option<String>> {
    Ok(match target.as_deref() {
        Some("wasip2" | "wasm32-wasip2") => Some(Flavour::P2.cli_name().to_string()),
        Some("wasm32-wasip1") => Some(Flavour::P1.cli_name().to_string()),
        Some(name @ ("wasip3" | "wasm32-wasip3")) => anyhow::bail!(
            "--target {name}: Perry cannot emit WASI 0.3 commands yet. Use \
             --target wasi (alias wasip2, wasm32-wasip2) for a WASI 0.2 component, \
             or --target wasip1 (alias wasm32-wasip1) for a WASI preview 1 module."
        ),
        _ => target,
    })
}

#[cfg(test)]
mod tests {
    use super::{canonical_target, Flavour};

    fn canon(name: &str) -> Option<String> {
        canonical_target(Some(name.to_string())).unwrap()
    }

    #[test]
    fn standard_wasi_triples_select_perry_target_names() {
        for name in ["wasi", "wasip2", "wasm32-wasip2"] {
            assert_eq!(canon(name).as_deref(), Some("wasi"), "{name}");
            assert_eq!(
                Flavour::from_target(&canon(name).unwrap()),
                Some(Flavour::P2)
            );
        }
        for name in ["wasip1", "wasm32-wasip1"] {
            assert_eq!(canon(name).as_deref(), Some("wasip1"), "{name}");
            assert_eq!(
                Flavour::from_target(&canon(name).unwrap()),
                Some(Flavour::P1)
            );
        }
    }

    #[test]
    fn other_targets_pass_through_unchanged() {
        assert_eq!(canonical_target(None).unwrap(), None);
        for name in ["linux", "web", "wasm", "ios-simulator"] {
            assert_eq!(canon(name).as_deref(), Some(name));
        }
    }

    #[test]
    fn wasi_0_3_names_fail_instead_of_linking_for_the_host() {
        for name in ["wasip3", "wasm32-wasip3"] {
            let err = canonical_target(Some(name.to_string()))
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("WASI 0.3") && err.contains("--target wasi"),
                "{err}"
            );
        }
    }
}
