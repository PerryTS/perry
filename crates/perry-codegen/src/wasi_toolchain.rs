//! WASI native EH requires LLVM's EHPadStack fix before any code generation.

use anyhow::{anyhow, bail, Result};

const MINIMUM: (u32, u32, u32) = (22, 1, 5);
const REQUIREMENT: &str = "--target wasi requires LLVM >= 22.1.5: LLVM issue #194184 \
    (EHPadStack calculation in fixCallUnwindMismatches) was fixed by \
    5c9647b8e680 in 22.1.5. Rebuild Perry with a supported LLVM for the \
    in-process backend, or update the explicitly selected external clang.";

/// Check the backend that will actually emit WASI objects, before lowering
/// modules or consulting object caches. Never select a different backend.
pub fn check() -> Result<()> {
    if crate::linker::inprocess_requested(false) {
        check_linked("wasm32-unknown-wasip2")
    } else {
        let clang = crate::linker::find_clang()
            .ok_or_else(|| anyhow!("{REQUIREMENT} No external clang found."))?;
        check_external(&clang)
    }
}

pub(crate) fn check_linked(target: &str) -> Result<()> {
    // Native code generation retains its existing toolchain requirements.
    if !target.starts_with("wasm32") {
        return Ok(());
    }
    let (mut major, mut minor, mut patch) = (0, 0, 0);
    unsafe { llvm_sys::core::LLVMGetVersion(&mut major, &mut minor, &mut patch) };
    check_version((major, minor, patch), "linked in-process LLVM")
}

pub(crate) fn check_external(clang: &std::path::Path) -> Result<()> {
    let banner = crate::linker::clang_version_output(clang)
        .ok_or_else(|| anyhow!("{REQUIREMENT} Cannot query {} --version.", clang.display()))?;
    let version = parse_version(&banner).ok_or_else(|| {
        anyhow!(
            "{REQUIREMENT} Cannot determine LLVM version from {} --version: {banner}",
            clang.display()
        )
    })?;
    check_version(version, &clang.display().to_string())
}

fn parse_version(banner: &str) -> Option<(u32, u32, u32)> {
    let version = banner
        .lines()
        .find_map(|line| line.split_once("clang version ").map(|(_, v)| v))?;
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch: String = parts
        .next()?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    Some((major, minor, patch.parse().ok()?))
}

fn check_version(version: (u32, u32, u32), backend: &str) -> Result<()> {
    if version < MINIMUM {
        let (major, minor, patch) = version;
        bail!("{REQUIREMENT} Found {backend} {major}.{minor}.{patch}.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_eh_version_requirement() {
        for version in [(21, 9, 9), (22, 0, 9), (22, 1, 4)] {
            let error = check_version(version, "test LLVM").unwrap_err().to_string();
            for required in ["--target wasi", ">= 22.1.5", "#194184", "5c9647b8e680"] {
                assert!(error.contains(required), "{error}");
            }
        }
        for version in [(22, 1, 5), (22, 1, 8), (22, 2, 0), (23, 0, 0)] {
            check_version(version, "test LLVM").unwrap();
        }
        check_linked("x86_64-unknown-linux-gnu").unwrap();
    }

    #[test]
    fn wasm_eh_external_version_banner() {
        for banner in [
            "clang version 22.1.5",
            "Ubuntu clang version 22.1.8 (++2026)",
            "Apple clang version 22.1.5 (clang-1)",
            "clang version 22.1.8git",
        ] {
            check_version(parse_version(banner).unwrap(), "test clang").unwrap();
        }
        assert!(
            check_version(parse_version("clang version 22.1.4").unwrap(), "test clang").is_err()
        );
        for banner in [
            "unknown",
            "clang version 22",
            "clang version 22.1",
            "clang version 22.1.x",
        ] {
            assert!(parse_version(banner).is_none(), "{banner}");
        }
    }

    #[test]
    fn wasm_eh_glob_ehpadstack_codegen() {
        // Extracted, already optimized Storage glob __620: preserve its actual
        // optimized CFG, including 49 retained invokes and 6 pads. LLVM #194184 fixed the backend
        // depth accounting this shape exposed; do not replace it with a toy.
        let ir = include_str!("../tests/fixtures/wasm-eh-glob-620.ll");
        assert_eq!(ir.matches(" invoke ").count(), 49);
        assert_eq!(ir.matches("catchpad within").count(), 6);
        let objects = crate::inprocess::compile_ll_to_object_inprocess(
            ir,
            "wasm32-unknown-wasip2",
            &["-O3".into()],
            "wasm-eh-glob-620.ll",
            false,
        )
        .expect("LLVM >= 22.1.5 must compile the glob EHPadStack regression witness");
        assert!(!objects.is_empty());
        for object in objects {
            assert!(object.starts_with(b"\0asm\x01\0\0\0"));
            assert!(object.len() > 8);
        }
    }
}
