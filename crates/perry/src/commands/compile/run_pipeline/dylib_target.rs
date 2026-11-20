//! Host toolchain validation and Apple SDK selection for shared-library links.
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

/// (#6222) Apple SDK + clang triple for a cross-compiled **shared library** link.
///
/// The dylib path links with the host `cc` driver and does NOT link the runtime
/// (symbols resolve from the host at `dlopen` time), so nothing else in that path
/// consults `--target`. Without these flags the linker happily consumes a correctly
/// cross-compiled iOS object and emits a **host-arch** dylib — the failure the user
/// sees is a link error or a dylib that will never load on device.
pub(super) fn apple_dylib_cross_target(target: &str) -> Option<(&'static str, &'static str)> {
    Some(match target {
        "ios" => ("iphoneos", "arm64-apple-ios17.0"),
        "ios-simulator" => ("iphonesimulator", "arm64-apple-ios17.0-simulator"),
        "tvos" => ("appletvos", "arm64-apple-tvos17.0"),
        "tvos-simulator" => ("appletvsimulator", "arm64-apple-tvos17.0-simulator"),
        "watchos" => ("watchos", "arm64_32-apple-watchos10.0"),
        "watchos-simulator" => ("watchsimulator", "arm64-apple-watchos10.0-simulator"),
        "visionos" => ("xros", "arm64-apple-xros1.0"),
        "visionos-simulator" => ("xrsimulator", "arm64-apple-xros1.0-simulator"),
        _ => return None,
    })
}

/// Resolve an Apple SDK sysroot via `xcrun`.
pub(super) fn apple_sdk_sysroot(sdk: &str) -> Result<PathBuf> {
    let out = Command::new("xcrun")
        .args(["--sdk", sdk, "--show-sdk-path"])
        .output()
        .with_context(|| {
            format!("Failed to invoke `xcrun --sdk {sdk} --show-sdk-path` (is Xcode installed?)")
        })?;
    if !out.status.success() {
        bail!(
            "`xcrun --sdk {sdk} --show-sdk-path` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() {
        bail!("`xcrun --sdk {sdk} --show-sdk-path` returned nothing");
    }
    Ok(PathBuf::from(path))
}

/// (#6222) Reject a `--target` the dylib link step cannot honour on this host,
/// instead of silently linking the cross object into a host-arch shared library.
/// The executable path already fails early and cleanly (it has to find a
/// per-target `libperry_runtime.a`); the dylib path links no runtime, so it had
/// no such check and produced a host binary in silence.
pub(super) fn verify_dylib_target_linkable(target: Option<&str>) -> Result<()> {
    let Some(t) = target else { return Ok(()) };
    let host_ok = match t {
        "windows" | "windows-winui" | "windows-x86_64" | "windows-aarch64" | "windows-arm64" => {
            true
        }
        t if t.starts_with("linux") => cfg!(target_os = "linux"),
        t if apple_dylib_cross_target(t).is_some() => cfg!(target_os = "macos"),
        _ => false,
    };
    if !host_ok {
        bail!(
            "--output-type dylib cannot link for target \"{t}\" on this host.\n\
             The object is cross-compiled correctly, but the shared-library link step \
             runs the host toolchain, so it would emit a host-architecture dylib.\n\
             Apple targets (ios, ios-simulator, tvos, watchos, visionos and their \
             simulators) require a macOS host with Xcode."
        );
    }
    Ok(())
}
