//! The glibc a Linux link resolves against, for `process.report`'s
//! `header.glibcVersionCompiler`.
//!
//! Node reports there the glibc its binary was built against. A Perry program
//! is built by its link, so the value is the glibc of the C library that link
//! uses: the target sysroot's when cross-linking, this host's when linking
//! natively. The compiler hands it to the runtime as the absolute symbol
//! `perry_glibc_version_compiler = major * 1000 + minor`.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{anyhow, Result};
// `Context::context` is only reached from the linux-gnu libc probe below.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use anyhow::Context;

/// The symbol the runtime reads (`process/report.rs`).
const SYMBOL: &str = "perry_glibc_version_compiler";

/// The linker argument that records the link's glibc version.
///
/// `driver` is the link driver as primed for the target (program, target
/// triple, sysroot and search flags); `native` says whether the link uses
/// this host's C library. A native link on a glibc host reads this process's
/// glibc. Every other link asks the driver which `<features.h>` it sees, so a
/// cross link reports the sysroot's glibc and never the link host's.
pub(super) fn glibc_version_defsym(driver: &Command, native: bool) -> Result<String> {
    let (major, minor) = link_glibc_version(driver, native)?;
    Ok(format!("-Wl,--defsym={SYMBOL}={}", major * 1000 + minor))
}

fn link_glibc_version(driver: &Command, native: bool) -> Result<(u32, u32)> {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    if native {
        return host_glibc_version();
    }
    let _ = native;
    driver_glibc_version(driver)
}

/// This process's glibc: what a native link on this host resolves against.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn host_glibc_version() -> Result<(u32, u32)> {
    // SAFETY: glibc returns a pointer to a static NUL-terminated string.
    let version = unsafe { std::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) }
        .to_str()
        .context("gnu_get_libc_version is not UTF-8")?;
    let mut parts = version.split('.');
    let mut number = || parts.next().and_then(|part| part.parse::<u32>().ok());
    match (number(), number()) {
        (Some(major), Some(minor)) => Ok((major, minor)),
        _ => Err(anyhow!("unexpected glibc version {version:?}")),
    }
}

/// The glibc version `<features.h>` declares for `driver`'s target: the
/// preprocessor runs with the same program, arguments and environment as the
/// link, so it reads the headers of the sysroot the link uses.
fn driver_glibc_version(driver: &Command) -> Result<(u32, u32)> {
    let program = driver.get_program().to_string_lossy().into_owned();
    let mut probe = Command::new(driver.get_program());
    probe.args(driver.get_args());
    for (key, value) in driver.get_envs() {
        match value {
            Some(value) => probe.env(key, value),
            None => probe.env_remove(key),
        };
    }
    if let Some(dir) = driver.get_current_dir() {
        probe.current_dir(dir);
    }
    probe
        .args(["-E", "-dM", "-x", "c", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let unknown = |reason: String| {
        anyhow!(
            "cannot determine the glibc version of the Linux link target: {reason}. \
             process.report needs it (header.glibcVersionCompiler); install the target's \
             C library headers (features.h) where the link driver `{program}` finds them"
        )
    };
    let mut child = probe
        .spawn()
        .map_err(|error| unknown(format!("`{program}` did not start: {error}")))?;
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(b"#include <features.h>\n")
        .map_err(|error| unknown(format!("writing to `{program}` failed: {error}")))?;
    let output = child
        .wait_with_output()
        .map_err(|error| unknown(format!("`{program}` failed: {error}")))?;
    if !output.status.success() {
        return Err(unknown(format!(
            "`{program} -E` on <features.h> failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    glibc_version_from_macros(&String::from_utf8_lossy(&output.stdout)).ok_or_else(|| {
        unknown("its <features.h> defines no __GLIBC__/__GLIBC_MINOR__ (not glibc)".into())
    })
}

/// `__GLIBC__` and `__GLIBC_MINOR__` from `-dM` preprocessor output.
fn glibc_version_from_macros(macros: &str) -> Option<(u32, u32)> {
    let mut major = None;
    let mut minor = None;
    for line in macros.lines() {
        let mut words = line.split_whitespace();
        if words.next() != Some("#define") {
            continue;
        }
        let (Some(name), Some(value), None) = (words.next(), words.next(), words.next()) else {
            continue;
        };
        match name {
            "__GLIBC__" => major = value.parse().ok(),
            "__GLIBC_MINOR__" => minor = value.parse().ok(),
            _ => {}
        }
    }
    Some((major?, minor?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_glibc_macros() {
        assert_eq!(
            glibc_version_from_macros(
                "#define __GLIBC_MINOR__ 39\n#define X 1\n#define __GLIBC__ 2\n"
            ),
            Some((2, 39))
        );
        assert_eq!(glibc_version_from_macros("#define __GLIBC__ 2\n"), None);
        assert_eq!(glibc_version_from_macros("#define __MUSL__ 1\n"), None);
    }

    fn cc_available() -> bool {
        Command::new("cc")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    /// A Linux link driver primed with `--sysroot`, as the cross branches
    /// prime it.
    fn cross_driver(sysroot: &std::path::Path) -> Command {
        let mut driver = Command::new("cc");
        #[cfg(not(target_os = "linux"))]
        driver.arg("-target").arg("x86_64-unknown-linux-gnu");
        driver.arg(format!("--sysroot={}", sysroot.display()));
        driver
    }

    /// A cross link reports the sysroot's glibc, not the link host's: the fake
    /// sysroot declares glibc 2.17, which no current host runs.
    #[test]
    fn a_cross_link_reports_the_sysroot_glibc() {
        if !cc_available() {
            eprintln!("SKIP: no cc");
            return;
        }
        let sysroot = tempfile::tempdir().expect("tempdir");
        let include = sysroot.path().join("usr/include");
        std::fs::create_dir_all(&include).expect("create include dir");
        std::fs::write(
            include.join("features.h"),
            "#define __GLIBC__ 2\n#define __GLIBC_MINOR__ 17\n",
        )
        .expect("write features.h");
        let driver = cross_driver(sysroot.path());
        assert_eq!(
            glibc_version_defsym(&driver, false).expect("sysroot glibc"),
            "-Wl,--defsym=perry_glibc_version_compiler=2017"
        );
    }

    /// A sysroot without glibc headers is a compile error, never a guess.
    #[test]
    fn a_sysroot_without_glibc_headers_is_an_error() {
        if !cc_available() {
            eprintln!("SKIP: no cc");
            return;
        }
        let sysroot = tempfile::tempdir().expect("tempdir");
        let error = glibc_version_defsym(&cross_driver(sysroot.path()), false)
            .expect_err("no features.h in the sysroot");
        assert!(
            error
                .to_string()
                .contains("cannot determine the glibc version"),
            "{error}"
        );
    }

    /// A native link on a glibc host reports this host's glibc.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[test]
    fn a_native_link_reports_the_host_glibc() {
        let (major, minor) = host_glibc_version().expect("host glibc");
        assert_eq!(
            glibc_version_defsym(&Command::new("cc"), true).expect("host glibc"),
            format!(
                "-Wl,--defsym=perry_glibc_version_compiler={}",
                major * 1000 + minor
            )
        );
    }
}
