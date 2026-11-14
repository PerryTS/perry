use std::env;
use std::fs;
use std::path::{Path, PathBuf};

// Perry: mimalloc's libc calls through the GOT (perry/noplt.rs).
#[path = "perry/noplt.rs"]
mod perry_noplt;

fn main() {
    let mut build = cc::Build::new();

    let version = if env::var("CARGO_FEATURE_V2").is_ok() {
        "v2"
    } else {
        "v3"
    };

    let cargo_manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let include_root = Path::new(&cargo_manifest_dir)
        .join("c_src")
        .join("mimalloc")
        .join(version);
    let include_dir = include_root
        .join("include")
        .to_str()
        .expect("include path is not valid UTF-8")
        .to_string();
    let static_source = include_root.join("src").join("static.c");

    // Make the include directory available to consumers via the `DEP_MIMALLOC_INCLUDE_DIR`
    // environment variable.
    println!("cargo:INCLUDE_DIR={include_dir}");

    build.include(format!("c_src/mimalloc/{version}/include"));
    build.include(format!("c_src/mimalloc/{version}/src"));

    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("target_os not defined!");
    let target_family = env::var("CARGO_CFG_TARGET_FAMILY").expect("target_family not defined!");
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let target_vendor = env::var("CARGO_CFG_TARGET_VENDOR").expect("target_vendor not defined!");
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").expect("target_arch not defined!");
    let cargo_debug = env::var("DEBUG")
        .map(|value| value == "true" || value == "1")
        .unwrap_or(false);
    let debug_enabled = env::var_os("CARGO_FEATURE_DEBUG").is_some()
        || (env::var_os("CARGO_FEATURE_DEBUG_IN_DEBUG").is_some() && cargo_debug);

    if target_family != "windows" {
        build.flag("-Wno-error=date-time");
    }

    if target_env == "msvc" {
        // Mimalloc expects the MSVC/clang-cl build to use the C++ atomics path.
        build.cpp(true);
        build.std("c++17");
        build.flag_if_supported("/Zc:__cplusplus");

        let wrapper =
            PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set")).join("mimalloc-static.cc");
        let include = static_source.to_string_lossy().replace('\\', "/");
        fs::write(&wrapper, format!("#include \"{include}\"\n"))
            .expect("failed to write mimalloc C++ wrapper");
        build.file(wrapper);
    } else {
        build.file(&static_source);
    }

    let compiler = build.get_compiler();

    if env::var_os("CARGO_FEATURE_OVERRIDE").is_some() {
        // Overriding malloc is only available on windows in shared mode, but we
        // only ever build a static lib.
        if target_family != "windows" {
            build.define("MI_MALLOC_OVERRIDE", None);
        }
        if target_vendor == "apple" {
            build.define("MI_OSX_ZONE", Some("1"));
            build.define("MI_OSX_INTERPOSE", Some("1"));
        }
        if !compiler.is_like_msvc() {
            build.flag_if_supported("-fno-builtin-malloc");
        }
    }

    if env::var_os("CARGO_FEATURE_SECURE").is_some() {
        build.define("MI_SECURE", "4");
    }

    if target_os == "windows" && env::var_os("CARGO_FEATURE_WIN_DIRECT_TLS").is_some() {
        build.define("MI_WIN_DIRECT_TLS", "1");
    }

    let dynamic_tls = env::var("CARGO_FEATURE_LOCAL_DYNAMIC_TLS").is_ok();

    if target_family == "unix" && target_os != "haiku" {
        if dynamic_tls {
            build.flag_if_supported("-ftls-model=local-dynamic");
        } else {
            build.flag_if_supported("-ftls-model=initial-exec");
        }
    }

    if (target_os == "linux" || target_os == "android")
        && env::var_os("CARGO_FEATURE_NO_THP").is_some()
    {
        build.define("MI_NO_THP", "1");
    }

    if debug_enabled {
        build.define("MI_DEBUG", "3");
        build.define("MI_SHOW_ERRORS", "1");
    } else {
        // Remove heavy debug assertions etc.
        build.define("MI_DEBUG", "0");
        if !cargo_debug {
            build.define("MI_BUILD_RELEASE", None);
            build.define("NDEBUG", None);
        }
    }

    // Perry: on x86_64 glibc Linux, mimalloc's calls of the libc functions
    // that Rust code also binds through the GOT go through the GOT too
    // (perry/perry_noplt_libc.h).
    let perry_noplt = perry_noplt::configure(&mut build, &compiler, &cargo_manifest_dir);

    build.compile("mimalloc");

    if perry_noplt {
        perry_noplt::verify(Path::new(&env::var("OUT_DIR").expect("OUT_DIR not set")));
    }

    // on armv6 we need to link with libatomic
    if target_os == "linux" && target_arch == "arm" {
        // Embrace the atomic capability library across various platforms.
        // For instance, on certain platforms, llvm has relocated the atomic of the arm32 architecture to libclang_rt.builtins.a
        // while some use libatomic.a, and others use libatomic_ops.a.
        let atomic_name = env::var("DEP_ATOMIC").unwrap_or("atomic".to_owned());
        println!("cargo:rustc-link-lib={}", atomic_name);
    }

    // Link with libs needed on Windows
    if target_os == "windows" {
        // https://github.com/microsoft/mimalloc/blob/af21001f7a65eafb8fb16460b018ebf9d75e2ad8/CMakeLists.txt#L487
        let libs = ["psapi", "shell32", "user32", "advapi32", "bcrypt"];

        for lib in libs {
            println!("cargo:rustc-link-lib={}", lib);
        }
    }
}
