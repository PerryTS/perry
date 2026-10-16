use std::process::Command;

/// (#1529) Android's `libperry_app.so` is loaded via `dlopen`, so its TLS
/// relocations must use the global-dynamic model — the aarch64-linux-android
/// default (Initial-Executable) crashes at load with
/// `TLS symbol "(null)" ... using IE access model`. The model is selected by a
/// `tls-model` rustc flag, but that flag is exposed as a stable `-C` codegen
/// option on some toolchains and is still nightly-gated (`-Z`) on others.
/// Passing the `-C` form to a toolchain that only knows the `-Z` form aborts
/// *every* Android build with `error: unknown codegen option: tls-model`.
/// (This slipped past CI because release CI builds the runtime libs with plain
/// `cargo build` and never compiles a full Android app through this path.)
///
/// Probe the active rustc and return the spelling it accepts. When only the
/// `-Z` form is available, also set `RUSTC_BOOTSTRAP=1` on `cmd` so the gated
/// flag is honored on a stable toolchain without requiring a nightly install.
pub(crate) fn android_global_dynamic_tls_rustflag(cmd: &mut Command) -> &'static str {
    let c_form_supported = Command::new("rustc")
        .args(["-C", "help"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("tls-model"))
        .unwrap_or(false);
    if c_form_supported {
        "-C tls-model=global-dynamic"
    } else {
        cmd.env("RUSTC_BOOTSTRAP", "1");
        "-Z tls-model=global-dynamic"
    }
}
