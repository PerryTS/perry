fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ENV");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_VENDOR");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let vendor = std::env::var("CARGO_CFG_TARGET_VENDOR").unwrap_or_default();
    let backend = match os.as_str() {
        "linux" | "android" => "epoll",
        _ if vendor == "apple" => "kqueue",
        "freebsd" | "openbsd" | "netbsd" | "dragonfly" => "kqueue",
        "windows" => "iocp",
        "wasi" if env == "p2" => "wasi_p2",
        "wasi" if env == "p3" => "wasi_p3",
        "unknown" if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") => "web",
        _ => "unsupported",
    };
    println!("cargo:rustc-cfg=turnloop_backend=\"{backend}\"");
    println!("cargo:rustc-env=TURNLOOP_BACKEND={backend}");
}
