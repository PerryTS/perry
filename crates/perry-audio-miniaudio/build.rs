use std::env;

fn main() {
    // Re-run if vendor sources change.
    println!("cargo:rerun-if-changed=vendor/miniaudio.h");
    println!("cargo:rerun-if-changed=vendor/miniaudio_impl.c");

    let target = env::var("TARGET").unwrap_or_default();

    let supported =
        target.contains("linux") || target.contains("android") || target.contains("windows");
    if !supported {
        return;
    }

    // This backend supports Linux, Windows, and Android. Apple targets use
    // the AVAudioEngine backend in their platform UI crates.
    let mut build = cc::Build::new();
    build
        .file("vendor/miniaudio_impl.c")
        .include("vendor")
        // miniaudio enables zero-init via memset which clang on -Wall flags
        // verbosely; silence the noise so logs stay focused on Perry.
        .flag_if_supported("-Wno-unused-function")
        .flag_if_supported("-Wno-unused-variable")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-implicit-function-declaration")
        .flag_if_supported("-Wno-deprecated-declarations");

    // miniaudio backend / link-directive selection. Each platform picks
    // its native audio API at runtime, but we need to declare the
    // system libraries miniaudio expects to be linked.
    if target.contains("linux") && !target.contains("android") {
        // PulseAudio / PipeWire / ALSA are dynamically loaded by
        // miniaudio via dlopen — only libdl + libpthread + libm
        // need to be linked at build time.
        println!("cargo:rustc-link-lib=dl");
        println!("cargo:rustc-link-lib=m");
        println!("cargo:rustc-link-lib=pthread");
    } else if target.contains("android") {
        // miniaudio uses AAudio on API 26+ (dlopened from libaaudio.so)
        // and OpenSL ES (libOpenSLES.so) on older Android — both come
        // with the NDK. AAudio is dlopened so the link line only needs
        // OpenSL ES.
        println!("cargo:rustc-link-lib=OpenSLES");
        println!("cargo:rustc-link-lib=log");
    } else if target.contains("windows") {
        // miniaudio falls back through WASAPI -> DirectSound -> WinMM,
        // all of which are part of the base Win32 API and resolved
        // through ole32 / winmm at link time. The `windows` crate
        // already pulls these in for perry-ui-windows but make the
        // requirement explicit here so the rlib stands on its own.
        println!("cargo:rustc-link-lib=ole32");
        println!("cargo:rustc-link-lib=winmm");
    }

    build.compile("miniaudio_impl");
}
