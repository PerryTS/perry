#!/usr/bin/env python3
"""Device regression for #10561 using the real Android template and a JNI test app.

Requires an idle arm64 Android emulator (API 29+), SDK/NDK, Gradle and JDK 17/21.
Usage: ANDROID_HOME=/path/to/sdk python3 scripts/test_android_night_mode.py emulator-5588
Only the explicitly selected device is used. Its night setting is restored and
our uniquely named package is uninstalled on exit. No Perry compiler build needed.
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
    package = "com.perry.nightmode.regression"

    def device(*args):
        return subprocess.check_output(adb + list(args), text=True, timeout=120).strip()

    if device("shell", "getprop", "ro.product.cpu.abi") != "arm64-v8a":
        raise SystemExit("The JNI fixture requires an arm64-v8a device")
    if int(device("shell", "getprop", "ro.build.version.sdk")) < 29:
        raise SystemExit("The night-mode regression requires Android API 29+")
    if f"package:{package}" in device("shell", "pm", "list", "packages", package).splitlines():
        raise SystemExit(f"Refusing to replace an existing {package} installation")
    original = device("shell", "cmd", "uimode", "night").split()[-1]
    if original not in ("auto", "yes", "no", "custom"):
        raise SystemExit(f"Unrecognized night mode: {original!r}")
    repo = Path(__file__).resolve().parents[1]
    fixtures = repo / "scripts/android-night-mode"
    project = Path(tempfile.mkdtemp(prefix="perry-night-mode-"))
    print(f"Build and test artifacts: {project}", flush=True)
    shutil.copytree(repo / "crates/perry-ui-android/template", project, dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("build", ".gradle", "local.properties", "jniLibs"))
    build = project / "app/build.gradle.kts"
    build.write_text(build.read_text().replace('applicationId = "com.perry.template"',
                                              f'applicationId = "{package}"'))
    manifest = project / "app/src/main/AndroidManifest.xml"
    manifest.write_text(manifest.read_text().replace("</manifest>",
        f'<instrumentation android:name="com.perry.app.NightModeTest" '
        f'android:targetPackage="{package}" />\n</manifest>'))
    shutil.copy(fixtures / "NightModeTest.java", project / "app/src/main/java/com/perry/app")
    candidates = sorted(glob.glob(str(sdk / "ndk/*/toolchains/llvm/prebuilt/*/bin/aarch64-linux-android24-clang")))
    compiler = os.environ.get("ANDROID_NDK_CLANG") or (candidates[-1] if candidates else None)
    if not compiler:
        raise SystemExit("Install an Android NDK or set ANDROID_NDK_CLANG")
    library = project / "app/src/main/jniLibs/arm64-v8a/libperry_app.so"
    library.parent.mkdir(parents=True)
    subprocess.run([compiler, "-shared", "-fPIC", str(fixtures / "native.c"), "-o", str(library)], check=True)
    env = dict(os.environ, ANDROID_HOME=str(sdk))
    subprocess.run(["gradle", "--no-daemon", "--console=plain", "assembleDebug"],
                   cwd=project, env=env, check=True)
    installed = False
    try:
        print(device("install", "-g", str(project / "app/build/outputs/apk/debug/app-debug.apk")))
        installed = True
        output = device("shell", "am", "instrument", "-w", "-r", f"{package}/com.perry.app.NightModeTest")
        (project / "instrumentation.log").write_text(output + "\n")
        print(output)
        if "PASS: three night-mode changes" not in output or "INSTRUMENTATION_CODE: 0" not in output:
            raise SystemExit("Night-mode regression failed")
    finally:
        device("shell", "cmd", "uimode", "night", original)
        if installed:
            device("uninstall", package)


if __name__ == "__main__":
    main()
