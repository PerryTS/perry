use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const WINAPPSDK_FOUNDATION_PACKAGE: &str = "Microsoft.WindowsAppSDK.Foundation";
const WINAPPSDK_FOUNDATION_VERSION: &str = "2.0.21";
const WINAPPSDK_INTERACTIVE_PACKAGE: &str = "Microsoft.WindowsAppSDK.InteractiveExperiences";
const WINAPPSDK_INTERACTIVE_VERSION: &str = "2.0.13";
const NUGET_URL_TEMPLATE: &str = "https://www.nuget.org/api/v2/package/{name}/{version}";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR set by cargo"));
    let temp_dir = out_dir.join("winappsdk-packages");
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    let foundation_dir = stage_package(
        WINAPPSDK_FOUNDATION_PACKAGE,
        WINAPPSDK_FOUNDATION_VERSION,
        &temp_dir,
    );
    let interactive_dir = stage_package(
        WINAPPSDK_INTERACTIVE_PACKAGE,
        WINAPPSDK_INTERACTIVE_VERSION,
        &temp_dir,
    );
    let target_dir = target_dir_from_out(&out_dir);
    let dest_dirs = [
        target_dir.clone(),
        target_dir.join("examples"),
        target_dir.join("deps"),
    ];
    for dest_dir in &dest_dirs {
        fs::create_dir_all(dest_dir).expect("Failed to create runtime asset directory");
    }

    let arch = format!("win-{}", target_arch());
    let bootstrap_src = foundation_dir
        .join(&arch)
        .join("native")
        .join("Microsoft.WindowsAppRuntime.Bootstrap.dll");
    let bootstrap_import_src = foundation_dir
        .join("native")
        .join(target_arch())
        .join("Microsoft.WindowsAppRuntime.Bootstrap.lib");

    if bootstrap_src.is_file() {
        println!("cargo:rerun-if-changed={}", bootstrap_src.display());
        for dest_dir in &dest_dirs {
            let dest = dest_dir.join("Microsoft.WindowsAppRuntime.Bootstrap.dll");
            if let Err(e) = fs::copy(&bootstrap_src, &dest) {
                println!(
                    "cargo:warning=Failed to copy bootstrap DLL to {}: {}",
                    dest.display(),
                    e
                );
            }
        }
    } else {
        println!(
            "cargo:warning=Microsoft.WindowsAppRuntime.Bootstrap.dll not found at {}",
            bootstrap_src.display()
        );
    }

    if bootstrap_import_src.is_file() {
        println!("cargo:rerun-if-changed={}", bootstrap_import_src.display());
        for dest_dir in &dest_dirs {
            let dest = dest_dir.join("Microsoft.WindowsAppRuntime.Bootstrap.lib");
            if let Err(e) = fs::copy(&bootstrap_import_src, &dest) {
                println!(
                    "cargo:warning=Failed to copy bootstrap import library to {}: {}",
                    dest.display(),
                    e
                );
            }
        }
    } else {
        println!(
            "cargo:warning=Microsoft.WindowsAppRuntime.Bootstrap.lib not found at {}",
            bootstrap_import_src.display()
        );
    }

    let pri_src = interactive_dir
        .join(&arch)
        .join("native")
        .join("Microsoft.UI.pri");

    if pri_src.is_file() {
        println!("cargo:rerun-if-changed={}", pri_src.display());
        for dest_dir in &dest_dirs {
            let dest = dest_dir.join("resources.pri");
            if let Err(e) = fs::copy(&pri_src, &dest) {
                println!(
                    "cargo:warning=Failed to copy framework PRI to {}: {}",
                    dest.display(),
                    e
                );
            }
        }
    } else {
        println!(
            "cargo:warning=Microsoft.UI.pri not found at {}. \
             XamlControlsResources will fail to load unless a resources.pri \
             file is present next to the executable.",
            pri_src.display()
        );
    }
}

fn stage_package(name: &str, version: &str, temp_dir: &Path) -> PathBuf {
    let nupkg_path = temp_dir.join(format!("{name}.{version}.nupkg"));
    let extract_dir = temp_dir.join(format!("{name}-{version}"));

    if !nupkg_path.is_file() {
        download_nupkg(name, version, &nupkg_path);
    }

    if !extract_dir.is_dir() {
        fs::create_dir_all(&extract_dir).expect("Failed to create extract directory");
        extract_archive(&nupkg_path, &extract_dir, &["--strip-components=1"]);
    }

    if !extract_dir.is_dir() || fs::read_dir(&extract_dir).map_or(true, |r| r.count() == 0) {
        println!(
            "cargo:warning=Extraction of {} to {} produced no files",
            nupkg_path.display(),
            extract_dir.display()
        );
    }

    extract_dir
}

fn download_nupkg(name: &str, version: &str, dest: &Path) {
    let url = NUGET_URL_TEMPLATE
        .replace("{name}", name)
        .replace("{version}", version);

    println!("cargo:warning=Downloading {name} version {version} from {url}");

    let curl_path = windows_system32().join("curl.exe");
    if !curl_path.is_file() {
        println!(
            "cargo:warning=curl.exe not found at {}",
            curl_path.display()
        );
        return;
    }

    let status = Command::new(&curl_path)
        .args([
            "-s",
            "-L",
            "-o",
            dest.to_str().expect("invalid dest path"),
            &url,
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:warning=Downloaded {name} version {version} successfully");
        }
        Ok(s) => {
            println!(
                "cargo:warning=Failed to download {name} version {version}: exit code {}",
                s.code().unwrap_or(-1)
            );
        }
        Err(e) => {
            println!("cargo:warning=Failed to run curl: {e}");
        }
    }
}

fn target_dir_from_out(out_dir: &Path) -> PathBuf {
    out_dir
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "build"))
        .and_then(Path::parent)
        .unwrap_or(out_dir)
        .to_path_buf()
}

fn windows_system32() -> PathBuf {
    PathBuf::from(env::var("SystemRoot").unwrap()).join("System32")
}

fn target_arch() -> &'static str {
    match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86") => "x86",
        _ => "x64",
    }
}

fn extract_archive(archive_path: &Path, dest_path: &Path, extra_args: &[&str]) {
    println!(
        "cargo:warning=Extracting archive {} to {}",
        archive_path.display(),
        dest_path.display()
    );

    let tar_path = windows_system32().join("tar.exe");
    if !tar_path.is_file() {
        println!("cargo:warning=tar.exe not found at {}", tar_path.display());
        return;
    }

    let status = Command::new(&tar_path)
        .args([
            "-xf",
            archive_path.to_str().expect("invalid archive path"),
            "-C",
            dest_path.to_str().expect("invalid destination path"),
        ])
        .args(extra_args)
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:warning=Extracted archive successfully");
        }
        Ok(s) => {
            println!(
                "cargo:warning=Failed to extract archive: {}",
                s.code().unwrap_or(-1)
            );
        }
        Err(e) => {
            println!("cargo:warning=Failed to run tar: {e}");
        }
    }
}
