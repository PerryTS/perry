#[path = "build/android_template.rs"]
mod android_template;

fn main() {
    android_template::emit().expect("failed to embed Android Gradle template");
    println!("cargo:rerun-if-changed=Cargo.toml");
    // CARGO_TARGET_DIR may place the compiler far from its source checkout.
    // Runtime discovery validates this hint before using it; release installs
    // can safely retain a path that no longer exists on the user's machine.
    let workspace =
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let workspace = workspace.canonicalize().expect("Perry workspace root");
    println!(
        "cargo:rerun-if-changed={}",
        workspace.join("Cargo.toml").display()
    );
    println!(
        "cargo:rustc-env=PERRY_BUILD_WORKSPACE_ROOT={}",
        workspace.display()
    );

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
