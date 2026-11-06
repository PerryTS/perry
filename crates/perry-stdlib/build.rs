// Match the runtime's instruments cohort so builtin input hooks cannot
// silently disappear from the instrumented archive.
fn main() {
    println!("cargo:rustc-check-cfg=cfg(perry_gc_instruments)");
    println!("cargo:rerun-if-env-changed=PERRY_RUNTIME_INSTRUMENTS");
    if std::env::var("PERRY_RUNTIME_INSTRUMENTS").is_ok_and(|v| v == "1") {
        println!("cargo:rustc-cfg=perry_gc_instruments");
    }
}
