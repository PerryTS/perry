//! The two WASI flavours behind `--target wasi` and `--target wasip1`.
//!
//! One helper owns the CLI name and the Rust/clang triple, so every driver
//! arm asks `Flavour::from_target` instead of matching target literals.

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
