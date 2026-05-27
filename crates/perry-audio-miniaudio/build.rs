use std::env;

fn main() {
    // Re-run if vendor sources change.
    println!("cargo:rerun-if-changed=vendor/miniaudio.h");
    println!("cargo:rerun-if-changed=vendor/miniaudio_impl.c");

    let target = env::var("TARGET").unwrap_or_default();

    // We compile miniaudio for every host, including Apple — Apple targets
    // get the rlib but never link the symbols (perry-ui-macos owns the
    // AVAudioEngine backend). Compiling here keeps `cargo check` /
    // `cargo build -p perry-audio-miniaudio` clean on developer Macs.
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
    } else if target.contains("apple") {
        // Apple targets compile this crate (so the workspace builds
        // cleanly on macOS developer machines) but should never link
        // its symbols — perry-ui-macos provides the real backend
        // backed by AVAudioEngine. Pull in the Core Audio frameworks
        // anyway because miniaudio_impl.c references them in its
        // Apple branch.
        println!("cargo:rustc-link-lib=framework=CoreAudio");
        println!("cargo:rustc-link-lib=framework=AudioToolbox");
        println!("cargo:rustc-link-lib=framework=AudioUnit");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    }

    build.compile("miniaudio_impl");
}
