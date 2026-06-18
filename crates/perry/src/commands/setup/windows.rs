use anyhow::{bail, Context, Result};
use console::style;
use dialoguer::Confirm;
use std::process::Command;

use super::*;

pub fn windows_wizard(accept_license: bool) -> Result<()> {
    use std::time::Instant;

    println!("  {}", style("Windows Toolchain Setup").bold());
    println!();
    println!("  This downloads the Microsoft CRT + Windows SDK libraries (via xwin)");
    println!("  so Perry can link Windows executables without Visual Studio Build Tools.");
    println!();

    // 1. Verify LLVM is present (provides clang for codegen + lld-link for linking).
    match perry_codegen::linker::find_clang() {
        Some(p) => println!("  {} LLVM found: {}", style("✓").green(), p.display()),
        None => bail!(
            "LLVM not found. Install it first, then rerun:\n  \
             winget install LLVM.LLVM    (or: choco install llvm / scoop install llvm)"
        ),
    }

    // 2. Locate xwin.
    let xwin = find_xwin_exe().ok_or_else(|| {
        anyhow::anyhow!(
            "xwin.exe not found. The Perry Windows release zip bundles it alongside \
         perry.exe. If you installed Perry from source (cargo install), install xwin \
         separately:\n  \
         cargo install xwin --locked --version 0.9.0"
        )
    })?;
    println!("  {} xwin found: {}", style("✓").green(), xwin.display());
    println!();

    // 3. Microsoft license acceptance. xwin's own URL (src/main.rs:269 in xwin 0.9.0).
    let license_url = "https://go.microsoft.com/fwlink/?LinkId=2086102";
    if !accept_license {
        println!(
            "  {} Microsoft Visual Studio Build Tools License",
            style("⚠").yellow().bold()
        );
        println!();
        println!("  The Microsoft CRT + Windows SDK libraries are redistributable under");
        println!("  the Microsoft Software License Terms. By proceeding you accept:");
        println!();
        println!("    {}", style(license_url).underlined().blue());
        println!();
        let accepted = Confirm::new()
            .with_prompt("  Do you accept the license?")
            .default(false)
            .interact()?;
        if !accepted {
            bail!("License not accepted — aborting.");
        }
    } else {
        println!(
            "  {} License accepted via --accept-license",
            style("ℹ").cyan()
        );
    }

    // 4. Output directory — %LOCALAPPDATA%\perry\windows-sdk (matches what
    //    find_perry_windows_sdk() in compile.rs probes at link time).
    let output_dir = dirs::data_local_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not resolve %LOCALAPPDATA%"))?
        .join("perry")
        .join("windows-sdk");

    println!();
    println!("  Output: {}", output_dir.display());
    println!("  Expect ~700 MB download / ~1.5 GB unpacked. Takes 2–4 minutes on a");
    println!("  typical connection. Partial downloads are resumable — safe to re-run.");
    println!();

    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("Failed to create {}", output_dir.display()))?;

    // 5. Run xwin. --disable-symlinks avoids noisy case-sensitivity symlinks on
    //    NTFS (xwin adds them on case-sensitive filesystems for windows.h vs
    //    Windows.h; Windows' NTFS is case-insensitive by default so they're a
    //    no-op). --accept-license since we already prompted.
    // xwin arg order: top-level flags (--accept-license, --arch) come BEFORE
    // the subcommand; splat-level flags (--output, --disable-symlinks) come AFTER.
    let start = Instant::now();
    let mut cmd = Command::new(&xwin);
    cmd.arg("--accept-license")
        .arg("--arch")
        .arg("x86_64")
        .arg("splat")
        .arg("--disable-symlinks")
        .arg("--output")
        .arg(&output_dir);

    let status = cmd
        .status()
        .with_context(|| format!("Failed to invoke {}", xwin.display()))?;
    if !status.success() {
        bail!(
            "xwin splat failed (status {}). The partial download at {} can be retried \
             safely — re-run `perry setup windows`.",
            status,
            output_dir.display()
        );
    }

    let elapsed = start.elapsed();
    println!();
    println!(
        "  {} Windows SDK ready at {}",
        style("✓").green().bold(),
        output_dir.display()
    );
    println!("    ({:.1}s)", elapsed.as_secs_f64());
    println!();
    println!(
        "  Try it:  {}",
        style("perry compile hello.ts && ./hello.exe").bold()
    );
    println!();
    println!("  Run `perry doctor` to verify the full toolchain.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Android wizard
// ---------------------------------------------------------------------------
