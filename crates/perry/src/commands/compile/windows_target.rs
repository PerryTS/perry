//! Windows target-name and architecture resolution.
//!
//! `windows` remains the native/default spelling: it follows the host
//! architecture on Windows and preserves the historical x64 default when
//! cross-compiling from another OS. Explicit architecture spellings make
//! cross-compilation deterministic.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WindowsTargetArch {
    X86_64,
    Aarch64,
}

impl WindowsTargetArch {
    pub(super) const fn rust_triple(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64-pc-windows-msvc",
            Self::Aarch64 => "aarch64-pc-windows-msvc",
        }
    }

    pub(super) const fn msvc_dir(self) -> &'static str {
        match self {
            Self::X86_64 => "x64",
            Self::Aarch64 => "arm64",
        }
    }

    pub(super) const fn xwin_dir(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }

    pub(super) const fn manifest_token(self) -> &'static str {
        match self {
            Self::X86_64 => "x64",
            Self::Aarch64 => "arm64",
        }
    }

    pub(super) const fn lock_token(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "arm64",
        }
    }
}

const fn native_windows_arch() -> WindowsTargetArch {
    if cfg!(target_arch = "aarch64") {
        WindowsTargetArch::Aarch64
    } else {
        WindowsTargetArch::X86_64
    }
}

/// Resolve a Perry target to its Windows machine architecture.
pub(super) fn windows_target_arch(target: Option<&str>) -> Option<WindowsTargetArch> {
    match target {
        Some("windows-aarch64") | Some("windows-arm64") => Some(WindowsTargetArch::Aarch64),
        Some("windows-x86_64") => Some(WindowsTargetArch::X86_64),
        Some("windows") | Some("windows-winui") => {
            if cfg!(target_os = "windows") {
                Some(native_windows_arch())
            } else {
                // Preserve the long-standing cross-host meaning of `windows`.
                Some(WindowsTargetArch::X86_64)
            }
        }
        None if cfg!(target_os = "windows") => Some(native_windows_arch()),
        _ => None,
    }
}

pub(super) fn is_windows_target(target: Option<&str>) -> bool {
    windows_target_arch(target).is_some()
}

/// True when the requested Windows target matches this compiler process.
/// Cross-architecture builds must never fall back to bare host artifact dirs.
pub(super) fn is_native_windows_target(target: Option<&str>) -> bool {
    cfg!(target_os = "windows") && windows_target_arch(target) == Some(native_windows_arch())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_windows_architectures_are_stable() {
        assert_eq!(
            windows_target_arch(Some("windows-x86_64")),
            Some(WindowsTargetArch::X86_64)
        );
        for target in ["windows-aarch64", "windows-arm64"] {
            assert_eq!(
                windows_target_arch(Some(target)),
                Some(WindowsTargetArch::Aarch64)
            );
            assert_eq!(
                windows_target_arch(Some(target)).map(WindowsTargetArch::rust_triple),
                Some("aarch64-pc-windows-msvc")
            );
        }
    }

    #[test]
    fn non_windows_targets_are_rejected() {
        assert_eq!(windows_target_arch(Some("linux")), None);
        assert!(!is_windows_target(Some("macos")));
    }

    #[test]
    fn explicit_other_architecture_is_not_native() {
        let other = if cfg!(target_arch = "aarch64") {
            "windows-x86_64"
        } else {
            "windows-aarch64"
        };
        assert!(!is_native_windows_target(Some(other)));
    }
}
