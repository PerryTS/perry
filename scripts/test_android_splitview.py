#!/usr/bin/env python3
"""Run #10553's production SplitView ABI/view regression on an explicit arm64 device.

Usage: ANDROID_HOME=/path/to/sdk python3 scripts/test_android_splitview.py emulator-5584
Requires Android SDK/NDK, Rust's aarch64-linux-android target, Gradle and JDK 17/21.
The fixture compiles the actual SplitView Rust modules, JNI bridge, widget registry
and panic boundaries with JNI alone, avoiding unrelated runtime dependencies.
It exercises the production Android template; it does not compile TypeScript.
"""
import glob
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    sdk = Path(os.environ.get("ANDROID_HOME") or os.environ["ANDROID_SDK_ROOT"])
    adb = [str(sdk / "platform-tools/adb"), "-s", sys.argv[1]]
    package = "com.perry.splitview.regression"

    def device(*args):
        return subprocess.check_output(adb + list(args), text=True, timeout=120).strip()

    if device("shell", "getprop", "ro.product.cpu.abi") != "arm64-v8a":
        raise SystemExit("The regression requires an arm64-v8a device")
    if f"package:{package}" in device("shell", "pm", "list", "packages", package).splitlines():
        raise SystemExit(f"Refusing to replace existing {package}")
    repo = Path(__file__).resolve().parents[1]
    source = repo / "crates/perry-ui-android/src"
    fixtures = repo / "scripts/android-splitview"
    work = Path(tempfile.mkdtemp(prefix="perry-splitview-"))
    print(f"Build and test artifacts: {work}", flush=True)
    project = work / "app-project"
    shutil.copytree(source.parent / "template", project,
                    ignore=shutil.ignore_patterns("build", ".gradle", "local.properties", "jniLibs"))
    build = project / "app/build.gradle.kts"
    build.write_text(build.read_text().replace('applicationId = "com.perry.template"',
                                              f'applicationId = "{package}"'))
    manifest = project / "app/src/main/AndroidManifest.xml"
    manifest.write_text(manifest.read_text().replace("</manifest>",
        f'<instrumentation android:name="com.perry.app.SplitViewTest" '
        f'android:targetPackage="{package}" />\n</manifest>'))
    shutil.copy(fixtures / "SplitViewTest.java", project / "app/src/main/java/com/perry/app")
    native = work / "native"
    (native / "src/widgets").mkdir(parents=True)
    (native / "Cargo.toml").write_text('''[package]
name = "perry-splitview-device-test"
version = "0.0.0"
edition = "2021"
[lib]
name = "perry_app"
crate-type = ["cdylib"]
[dependencies]
jni = "=0.22.4"
[profile.dev]
debug = 0
''')
    shutil.copy(fixtures / "native.rs", native / "src/lib.rs")
    shutil.copy(source / "jni_bridge.rs", native / "src")
    shutil.copy(source / "widgets/splitview.rs", native / "src/widgets")
    shutil.copy(source / "ffi/splitview.rs", native / "src/ffi.rs")
    registry = (source / "widgets/mod.rs").read_text()
    registry = registry[registry.index("use crate::jni_bridge::GlobalRef;"):registry.index("/// Set the hidden state")]
    (native / "src/widgets/mod.rs").write_text("pub mod splitview;\n" + registry)
    boundary = (source / "lib.rs").read_text()
    boundary = boundary[boundary.index("/// Catch panics from widget functions"):boundary.index("/// Called by the JVM")]
    (native / "src/panic_boundary.rs").write_text(boundary)
    candidates = sorted(glob.glob(str(sdk / "ndk/*/toolchains/llvm/prebuilt/*/bin/aarch64-linux-android24-clang")))
    compiler = os.environ.get("ANDROID_NDK_CLANG") or (candidates[-1] if candidates else None)
    if not compiler:
        raise SystemExit("Install an NDK or set ANDROID_NDK_CLANG")
    env = dict(os.environ, ANDROID_HOME=str(sdk), CARGO_TARGET_DIR=str(native / "target"),
               CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=compiler)
    subprocess.run(["cargo", "build", "--target", "aarch64-linux-android"], cwd=native, env=env, check=True)
    library = project / "app/src/main/jniLibs/arm64-v8a/libperry_app.so"
    library.parent.mkdir(parents=True)
    shutil.copy(native / "target/aarch64-linux-android/debug/libperry_app.so", library)
    subprocess.run(["gradle", "--no-daemon", "--console=plain", "assembleDebug"], cwd=project, env=env, check=True)
    installed = False
    try:
        print(device("install", "-g", str(project / "app/build/outputs/apk/debug/app-debug.apk")))
        installed = True
        output = device("shell", "am", "instrument", "-w", "-r", f"{package}/com.perry.app.SplitViewTest")
        (work / "instrumentation.log").write_text(output + "\n")
        print(output)
        if "PASS: production Rust SplitView ABI" not in output or "INSTRUMENTATION_CODE: 0" not in output:
            raise SystemExit("SplitView regression failed")
    finally:
        if installed:
            device("uninstall", package)


if __name__ == "__main__":
    main()
