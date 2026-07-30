fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");

    if std::env::var_os("CARGO_CFG_TARGET_OS").as_deref() != Some(std::ffi::OsStr::new("windows")) {
        return;
    }

    // Windows Explorer reads these fields from the executable's VERSIONINFO
    // resource. `WindowsResource::new` takes the file/product version from
    // CARGO_PKG_VERSION and the descriptive fields from Cargo.toml.
    winresource::WindowsResource::new()
        .compile()
        .expect("failed to compile perry.exe VERSIONINFO resource");
}
