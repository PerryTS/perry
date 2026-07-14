//! Android APK build, sign, install + on-device launch via Gradle/adb.

use super::*;

/// Build an Android APK from the compiled .so and install/launch on a device.
///
/// Steps:
/// 1. Copy the Gradle template from perry-ui-android/template/ to a temp dir
/// 2. Place the compiled .so in app/src/main/jniLibs/arm64-v8a/
/// 3. Update the applicationId in build.gradle.kts
/// 4. Run ./gradlew assembleDebug
/// 5. Sign and install the resulting APK
pub fn build_and_run_android(
    so_path: &Path,
    bundle_id: &str,
    serial: &str,
    format: OutputFormat,
) -> Result<()> {
    build_and_run_android_impl(so_path, bundle_id, serial, format, false)
}

/// Build + install a Wear OS APK. Wear OS is Android-on-a-watch, so this reuses
/// the exact same Gradle project, Kotlin bridge, and compiled `.so` as the phone
/// path (`build_and_run_android`) — the only differences are the watch
/// form-factor declarations applied by `apply_wear_overlay`: the
/// `android.hardware.type.watch` feature + the standalone meta-data + the
/// `androidx.wear` dependency.
pub fn build_and_run_wearos(
    so_path: &Path,
    bundle_id: &str,
    serial: &str,
    format: OutputFormat,
) -> Result<()> {
    build_and_run_android_impl(so_path, bundle_id, serial, format, true)
}

fn build_and_run_android_impl(
    so_path: &Path,
    bundle_id: &str,
    serial: &str,
    format: OutputFormat,
    wear: bool,
) -> Result<()> {
    // Find the perry workspace root to locate the Android template
    let workspace_root = super::super::compile::find_perry_workspace_root()
        .ok_or_else(|| anyhow!("Cannot find Perry workspace root — needed for Android template"))?;
    let template_dir = workspace_root.join("crates/perry-ui-android/template");
    if !template_dir.exists() {
        bail!("Android template not found at {}", template_dir.display());
    }

    // Create a build directory alongside the .so
    let build_dir = so_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("android-build");
    if build_dir.exists() {
        std::fs::remove_dir_all(&build_dir).ok();
    }

    if let OutputFormat::Text = format {
        println!();
        println!("Building Android APK...");
    }

    // Copy template to build directory
    copy_dir_recursive(&template_dir, &build_dir)
        .map_err(|e| anyhow!("Failed to copy Android template: {}", e))?;

    // Wear OS: overlay the watch form-factor onto the copied phone template
    // (manifest feature + standalone meta-data, Wear minSdk, androidx.wear dep).
    if wear {
        if let Err(e) = apply_wear_overlay(&build_dir, format) {
            bail!("Failed to apply Wear OS template overlay: {}", e);
        }
    }

    // Create jniLibs directory and copy .so
    let jni_dir = build_dir.join("app/src/main/jniLibs/arm64-v8a");
    std::fs::create_dir_all(&jni_dir)?;
    std::fs::copy(so_path, jni_dir.join("libperry_app.so"))
        .map_err(|e| anyhow!("Failed to copy .so to jniLibs: {}", e))?;

    // #1527 — bundle companion shared libraries. The compile step copies
    // third-party `crate-android` cdylib outputs (e.g. `libperry_play_billing.so`
    // from @perryts/play-billing) next to the main binary in the staging dir
    // (the "Copied companion library" log line). `libperry_app.so` records them
    // in DT_NEEDED, so the dynamic linker must find them in the same `lib/<abi>/`
    // dir inside the APK — Gradle bundles whatever lives in jniLibs/<abi>/.
    // Without this, the app crashes on launch with `UnsatisfiedLinkError:
    // dlopen failed: library "lib….so" not found`. Static-lib companions link
    // directly into libperry_app.so and have no `.so` here, so nothing to copy.
    let staging_dir = so_path.parent().unwrap_or(Path::new("."));
    if let Ok(entries) = std::fs::read_dir(staging_dir) {
        let mut bundled = 0usize;
        for entry in entries.flatten() {
            let path = entry.path();
            // Only sibling `.so` files; the main output was already copied as
            // libperry_app.so above, and the `path == *so_path` guard below
            // keeps it from being copied a second time under its own name
            // (#5740 made the default output `lib<stem>.so`, so it now does
            // carry a `.so` extension and would otherwise match here).
            let is_so = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e == "so")
                .unwrap_or(false);
            if !is_so || path == *so_path {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            match std::fs::copy(&path, jni_dir.join(name)) {
                Ok(_) => {
                    bundled += 1;
                    if let OutputFormat::Text = format {
                        println!("  Bundled companion library: {}", name.to_string_lossy());
                    }
                }
                Err(e) => {
                    if let OutputFormat::Text = format {
                        println!(
                            "Warning: failed to bundle companion library {}: {}",
                            name.to_string_lossy(),
                            e
                        );
                    }
                }
            }
        }
        if bundled > 0 {
            if let OutputFormat::Text = format {
                println!(
                    "  Bundled {} companion .so into jniLibs/arm64-v8a (#1527)",
                    bundled
                );
            }
        }
    }

    // Copy resource directories (assets/, logo/, etc.) into APK assets
    // Android loads ImageFile('assets/foo.png') from the APK's assets/ directory,
    // so we need 'assets/' inside 'app/src/main/assets/' → accessible as 'assets/foo.png'
    let project_root = so_path.parent().unwrap_or(Path::new("."));
    let apk_assets = build_dir.join("app/src/main/assets");
    std::fs::create_dir_all(&apk_assets)?;
    for dir_name in &["logo", "assets", "resources", "images"] {
        let resource_dir = project_root.join(dir_name);
        if resource_dir.is_dir() {
            let dest = apk_assets.join(dir_name);
            let _ = copy_dir_recursive(&resource_dir, &dest);
        }
    }

    // Update applicationId in build.gradle.kts
    let gradle_path = build_dir.join("app/build.gradle.kts");
    if gradle_path.exists() {
        let content = std::fs::read_to_string(&gradle_path)?;
        let updated = content.replace(
            "applicationId = \"com.perry.template\"",
            &format!("applicationId = \"{}\"", bundle_id),
        );
        std::fs::write(&gradle_path, updated)?;
    }

    // Issue #583: inject deep-link intent filters into AndroidManifest.xml
    // from package.json `perry.deepLinks`. Adds an
    // `<intent-filter android:autoVerify="true">` block per host for App
    // Links, plus a per-scheme `<intent-filter>` for custom schemes. The
    // singleTop launch mode is also enabled so foreground URL deliveries
    // route through `onNewIntent` instead of relaunching the Activity.
    let manifest_path = build_dir.join("app/src/main/AndroidManifest.xml");
    if manifest_path.exists() {
        if let Err(e) = inject_android_deeplinks(&manifest_path, project_root, format) {
            if let OutputFormat::Text = format {
                println!("Warning: deep-link intent filters not applied: {}", e);
            }
        }
    }

    // #1138 — write the configured Google Sign In server clientID
    // into a string resource the @perryts/google-auth Kotlin bridge
    // reads via R.string.google_auth_server_client_id.
    if let Err(e) = inject_google_auth_android_resources(&build_dir, project_root, format) {
        if let OutputFormat::Text = format {
            println!(
                "Warning: [google_auth] not applied to Android resources: {}",
                e
            );
        }
    }

    // #867 — inject the Google Mobile Ads `APPLICATION_ID` meta-data into
    // AndroidManifest.xml from `[ads].android_app_id` in perry.toml.
    if let Err(e) = inject_ads_android_manifest(&build_dir, project_root, format) {
        if let OutputFormat::Text = format {
            println!("Warning: [ads] not applied to AndroidManifest.xml: {}", e);
        }
    }

    // #1138 — when `@perryts/google-auth` is installed under
    // node_modules, copy its `crate-android/kotlin/*.kt` sources
    // into the Gradle project + merge its declared gradle deps so
    // the Kotlin bridge compiles into the produced APK.
    if let Err(e) = wire_native_lib_kotlin_sources(&build_dir, project_root, format) {
        if let OutputFormat::Text = format {
            println!("Warning: nativeLibrary Kotlin sources not wired: {}", e);
        }
    }

    // #1139 — build any [[widget]] entries declared in perry.toml
    // that ship a `glance_source` Kotlin tree. Copies sources into
    // the Gradle project, emits appwidget-provider XML, and injects
    // a <receiver> entry into AndroidManifest.xml.
    // `project_root` is where `package.json`/`perry.toml` live;
    // `bundle_id` is the applicationId we just rewrote into
    // build.gradle.kts so package + receiver namespacing match.
    let widget_input = project_root.join("perry.toml");
    let widget_input_path = if widget_input.exists() {
        widget_input
    } else {
        project_root.to_path_buf()
    };
    match super::super::compile::widget_build::build_declared_widgets_android(
        &widget_input_path,
        &build_dir,
        bundle_id,
        format,
    ) {
        Ok(n) if n > 0 => {
            if let OutputFormat::Text = format {
                println!(
                    "  Embedded {} Glance widget{} into the APK",
                    n,
                    if n == 1 { "" } else { "s" }
                );
            }
        }
        Ok(_) => {}
        Err(e) => {
            if let OutputFormat::Text = format {
                println!("Warning: Glance widget build path failed: {}", e);
            }
        }
    }

    // Generate gradle wrapper if not present
    let gradlew = build_dir.join("gradlew");
    if !gradlew.exists() {
        if let OutputFormat::Text = format {
            println!("Generating Gradle wrapper...");
        }
        let wrapper_status = Command::new("gradle")
            .arg("wrapper")
            .current_dir(&build_dir)
            .status();
        match wrapper_status {
            Ok(s) if s.success() => {}
            _ => bail!(
                "Failed to generate Gradle wrapper. Install Gradle: brew install gradle\n\
                 Or install Android Studio which includes Gradle."
            ),
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if gradlew.exists() {
            let mut perms = std::fs::metadata(&gradlew)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&gradlew, perms)?;
        }
    }

    if let OutputFormat::Text = format {
        println!("Running Gradle assembleDebug...");
    }

    // Run gradle build. `gradlew` is `build_dir.join("gradlew")`, which is a
    // RELATIVE path when the compile output (and thus build_dir) is relative —
    // e.g. `android-build/gradlew`. Spawning a relative program path that
    // contains a `/` while also setting `.current_dir(&build_dir)` resolves the
    // program against the *new* cwd, i.e. `android-build/android-build/gradlew`,
    // which fails with ENOENT. Canonicalize to an absolute path so the spawn is
    // independent of the child's working directory.
    let gradlew_abs = std::fs::canonicalize(&gradlew).unwrap_or_else(|_| gradlew.clone());
    let gradle_status = Command::new(&gradlew_abs)
        .arg("assembleDebug")
        .current_dir(&build_dir)
        .status()
        .map_err(|e| {
            anyhow!(
                "Failed to run Gradle: {}. Is the Android SDK/Gradle installed?",
                e
            )
        })?;

    if !gradle_status.success() {
        bail!("Gradle build failed. Check the output above for errors.");
    }

    // Find the APK
    let apk_path = build_dir.join("app/build/outputs/apk/debug/app-debug.apk");
    if !apk_path.exists() {
        bail!("APK not found at expected path: {}", apk_path.display());
    }

    if let OutputFormat::Text = format {
        println!("APK built: {}", apk_path.display());
    }

    // Install and launch
    install_and_launch_android(&apk_path, bundle_id, serial, format)
}

/// Turn the copied phone Gradle project into a Wear OS app in place.
///
/// Wear OS apps are ordinary Android apps with three extra declarations:
///   1. `<uses-feature android:name="android.hardware.type.watch">` — marks
///      the APK as a watch app (Play Store filtering + launcher placement).
///   2. `<meta-data com.google.android.wearable.standalone = true>` — the app
///      runs without a companion phone app.
///   3. `androidx.wear:wear` — pulls in `BoxInsetLayout` / swipe-to-dismiss so
///      round screens and the back gesture behave like a native Wear app.
/// minSdk is also raised to 30 (Wear OS 3), the floor Google Play requires for
/// watch APKs.
fn apply_wear_overlay(build_dir: &Path, format: OutputFormat) -> Result<()> {
    // --- AndroidManifest.xml ---
    let manifest_path = build_dir.join("app/src/main/AndroidManifest.xml");
    if manifest_path.exists() {
        let mut manifest = std::fs::read_to_string(&manifest_path)?;

        // 1. Watch hardware feature — required, right after the <manifest> tag.
        let manifest_open =
            "<manifest xmlns:android=\"http://schemas.android.com/apk/res/android\">";
        if manifest.contains(manifest_open) && !manifest.contains("android.hardware.type.watch") {
            manifest = manifest.replacen(
                manifest_open,
                &format!(
                    "{manifest_open}\n\n    <uses-feature android:name=\"android.hardware.type.watch\" android:required=\"true\" />"
                ),
                1,
            );
        }

        // 2. Standalone meta-data + the wearable shared library, inserted just
        //    before the existing Maps API key meta-data inside <application>.
        let maps_anchor =
            "        <meta-data\n            android:name=\"com.google.android.geo.API_KEY\"";
        if manifest.contains(maps_anchor) && !manifest.contains("wearable.standalone") {
            let wear_meta = "        <meta-data\n            android:name=\"com.google.android.wearable.standalone\"\n            android:value=\"true\" />\n        <uses-library\n            android:name=\"com.google.android.wearable\"\n            android:required=\"false\" />\n";
            manifest = manifest.replacen(maps_anchor, &format!("{wear_meta}{maps_anchor}"), 1);
        }

        std::fs::write(&manifest_path, &manifest)?;
    }

    // --- app/build.gradle.kts ---
    let gradle_path = build_dir.join("app/build.gradle.kts");
    if gradle_path.exists() {
        let mut gradle = std::fs::read_to_string(&gradle_path)?;
        // Wear OS 3 is the minSdk floor for watch APKs on Google Play.
        gradle = gradle.replace("minSdk = 24", "minSdk = 30");
        // Add the Wear support library (BoxInsetLayout, swipe-to-dismiss).
        gradle = inject_gradle_dependencies(&gradle, &["androidx.wear:wear:1.3.0".to_string()]);
        std::fs::write(&gradle_path, gradle)?;
    }

    if let OutputFormat::Text = format {
        println!("  Wear OS overlay: watch feature + standalone + androidx.wear (minSdk 30)");
    }
    Ok(())
}

/// Issue #583 — read `package.json` `perry.deepLinks` and rewrite the
/// AndroidManifest.xml inside the materialized template directory.
///
/// #1138 — write `[google_auth]` config from `perry.toml` into a
/// `res/values/google_auth.xml` string resource the
/// `@perryts/google-auth` Kotlin bridge reads at runtime. No-op when
/// the block is absent.
pub fn inject_google_auth_android_resources(
    build_dir: &Path,
    project_root: &Path,
    format: OutputFormat,
) -> Result<()> {
    let mut dir: PathBuf = project_root.to_path_buf();
    let mut perry_toml: Option<String> = None;
    for _ in 0..6 {
        let p = dir.join("perry.toml");
        if p.exists() {
            perry_toml = std::fs::read_to_string(&p).ok();
            break;
        }
        if !dir.pop() {
            break;
        }
    }
    let raw = match perry_toml {
        Some(r) => r,
        None => return Ok(()),
    };
    let doc: toml::Table = raw.parse()?;
    let ga = match doc.get("google_auth").and_then(|v| v.as_table()) {
        Some(g) => g,
        None => return Ok(()),
    };

    // Prefer server_client_id (the value the CredentialManager
    // GetGoogleIdOption wants), fall back to android_client_id.
    let client_id = ga
        .get("server_client_id")
        .and_then(|v| v.as_str())
        .or_else(|| ga.get("android_client_id").and_then(|v| v.as_str()));
    let Some(client_id) = client_id else {
        return Ok(());
    };

    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <resources>\n    \
             <string name=\"google_auth_server_client_id\" translatable=\"false\">{}</string>\n\
         </resources>\n",
        // Escape XML special chars in the clientID just in case (Google
        // clientIDs are alphanumeric + `.` + `-` so this is belt-and-
        // suspenders, but matches the rest of the perry.toml->xml flow).
        client_id
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    );

    let res_dir = build_dir.join("app/src/main/res/values");
    std::fs::create_dir_all(&res_dir)?;
    std::fs::write(res_dir.join("google_auth.xml"), xml)?;

    if let OutputFormat::Text = format {
        println!("  google_auth: wrote res/values/google_auth.xml (#1138)");
    }
    Ok(())
}

/// #867 — splice the Google Mobile Ads App ID into AndroidManifest.xml.
///
/// The SDK requires
/// `<meta-data android:name="com.google.android.gms.ads.APPLICATION_ID" .../>`
/// under `<application>`; `MobileAds.initialize()` throws at startup if
/// it's missing. Reads `[ads].android_app_id` from `perry.toml` and
/// inserts the element immediately before `</application>`. No-op when
/// the block / key is absent.
///
/// Idempotent: if the manifest already declares the APPLICATION_ID
/// meta-data (a rebuild into a reused build dir) we leave it untouched
/// rather than double-injecting.
pub fn inject_ads_android_manifest(
    build_dir: &Path,
    project_root: &Path,
    format: OutputFormat,
) -> Result<()> {
    let mut dir: PathBuf = project_root.to_path_buf();
    let mut perry_toml: Option<String> = None;
    for _ in 0..6 {
        let p = dir.join("perry.toml");
        if p.exists() {
            perry_toml = std::fs::read_to_string(&p).ok();
            break;
        }
        if !dir.pop() {
            break;
        }
    }
    let raw = match perry_toml {
        Some(r) => r,
        None => return Ok(()),
    };
    let doc: toml::Table = raw.parse()?;
    let ads = match doc.get("ads").and_then(|v| v.as_table()) {
        Some(a) => a,
        None => return Ok(()),
    };
    let Some(app_id) = ads.get("android_app_id").and_then(|v| v.as_str()) else {
        return Ok(());
    };

    let manifest_path = build_dir.join("app/src/main/AndroidManifest.xml");
    if !manifest_path.exists() {
        return Ok(());
    }
    let manifest = std::fs::read_to_string(&manifest_path)?;

    // Idempotency guard — the `</application>` splice below is not
    // idempotent on its own, so bail if the App ID meta-data is already
    // present.
    if manifest.contains("com.google.android.gms.ads.APPLICATION_ID") {
        return Ok(());
    }

    // App IDs are `ca-app-pub-<digits>~<digits>` — no XML specials — but
    // escape defensively to match the rest of the perry.toml->xml flow.
    let escaped = app_id
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    let meta = format!(
        "        <meta-data\n            android:name=\"com.google.android.gms.ads.APPLICATION_ID\"\n            android:value=\"{}\" />\n",
        escaped
    );

    let updated = if let Some(idx) = manifest.find("</application>") {
        let (head, tail) = manifest.split_at(idx);
        format!("{}{}{}", head, meta, tail)
    } else {
        return Err(anyhow!(
            "AndroidManifest.xml at `{}` has no </application> tag",
            manifest_path.display()
        ));
    };
    std::fs::write(&manifest_path, updated)?;

    if let OutputFormat::Text = format {
        println!("  ads: injected GoogleMobileAds APPLICATION_ID meta-data (#867)");
    }
    Ok(())
}

/// #1138 — when a `perry.nativeLibrary` package under
/// `node_modules/@*/<pkg>` declares `targets.android.kotlin_sources`
/// and/or `targets.android.gradle_dependencies`, copy the Kotlin
/// files into the Gradle project at `app/src/main/java/...` and
/// merge the deps into `app/build.gradle.kts`. The Kotlin file's
/// `package com.foo.bar` line is used to decide where to drop it.
///
/// Best-effort: missing node_modules / missing package / no
/// nativeLibrary block → no-op without error.
pub fn wire_native_lib_kotlin_sources(
    build_dir: &Path,
    project_root: &Path,
    format: OutputFormat,
) -> Result<()> {
    let node_modules = (|| -> Option<PathBuf> {
        let mut dir = project_root.to_path_buf();
        for _ in 0..6 {
            let nm = dir.join("node_modules");
            if nm.is_dir() {
                return Some(nm);
            }
            if !dir.pop() {
                break;
            }
        }
        None
    })();
    let Some(node_modules) = node_modules else {
        return Ok(());
    };

    let mut kotlin_copied = 0usize;
    let mut gradle_deps: Vec<String> = Vec::new();

    // Scan node_modules for packages declaring perry.nativeLibrary
    // with android kotlin_sources / gradle_dependencies. Limit to
    // one level of scoped (`@scope/pkg`) and unscoped packages —
    // matches the layout npm produces.
    let scan_dirs = std::fs::read_dir(&node_modules)?;
    let mut pkg_dirs: Vec<PathBuf> = Vec::new();
    for entry in scan_dirs.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if name.starts_with('@') {
            if let Ok(scoped) = std::fs::read_dir(&path) {
                for sub in scoped.flatten() {
                    pkg_dirs.push(sub.path());
                }
            }
        } else if !name.starts_with('.') {
            pkg_dirs.push(path);
        }
    }

    for pkg_dir in pkg_dirs {
        let pkg_json = pkg_dir.join("package.json");
        if !pkg_json.exists() {
            continue;
        }
        let data = match std::fs::read_to_string(&pkg_json) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let pkg_val: serde_json::Value = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let android = pkg_val
            .pointer("/perry/nativeLibrary/targets/android")
            .cloned();
        let Some(android) = android else { continue };

        // Kotlin sources: copy each into the Gradle project under
        // the package path declared by the file's `package` line.
        if let Some(arr) = android.get("kotlin_sources").and_then(|v| v.as_array()) {
            for entry in arr {
                let Some(rel) = entry.as_str() else { continue };
                let src = pkg_dir.join(rel);
                if !src.exists() {
                    if let OutputFormat::Text = format {
                        println!(
                            "  Warning: kotlin_sources entry missing: {} ({})",
                            src.display(),
                            pkg_dir.display()
                        );
                    }
                    continue;
                }
                let kt = std::fs::read_to_string(&src)?;
                let pkg_line = kt.lines().find(|l| l.trim_start().starts_with("package "));
                let pkg_path = pkg_line
                    .and_then(|l| l.trim().strip_prefix("package "))
                    .map(|s| s.trim_end_matches(';').trim().to_string())
                    .unwrap_or_else(|| "com.perryts.unknown".to_string())
                    .replace('.', "/");
                let dest_dir = build_dir.join("app/src/main/java").join(&pkg_path);
                std::fs::create_dir_all(&dest_dir)?;
                let dest = dest_dir.join(
                    src.file_name()
                        .map(|s| s.to_os_string())
                        .unwrap_or_default(),
                );
                std::fs::copy(&src, &dest)?;
                kotlin_copied += 1;
            }
        }

        // Gradle deps: collect for one final merge into build.gradle.kts.
        if let Some(arr) = android
            .get("gradle_dependencies")
            .and_then(|v| v.as_array())
        {
            for d in arr {
                if let Some(s) = d.as_str() {
                    gradle_deps.push(s.to_string());
                }
            }
        }
    }

    if !gradle_deps.is_empty() {
        let gradle = build_dir.join("app/build.gradle.kts");
        if gradle.exists() {
            let body = std::fs::read_to_string(&gradle)?;
            // Append before the final closing `}` of the dependencies
            // block. If the block is missing entirely, fall back to
            // appending a fresh block at file end.
            let injected = inject_gradle_dependencies(&body, &gradle_deps);
            std::fs::write(&gradle, injected)?;
        }
    }

    if (kotlin_copied > 0 || !gradle_deps.is_empty()) && matches!(format, OutputFormat::Text) {
        println!(
            "  nativeLibrary: wired {} Kotlin source(s) + {} gradle dep(s)",
            kotlin_copied,
            gradle_deps.len()
        );
    }
    Ok(())
}

pub fn inject_gradle_dependencies(body: &str, deps: &[String]) -> String {
    let block = "dependencies {";
    if let Some(start) = body.find(block) {
        // Find the matching `}` for the dependencies block.
        let after = start + block.len();
        let mut depth = 1usize;
        let bytes = body.as_bytes();
        let mut end = after;
        while end < bytes.len() && depth > 0 {
            match bytes[end] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            end += 1;
        }
        if depth == 0 {
            let mut lines = String::new();
            for d in deps {
                if !body.contains(d) {
                    lines.push_str(&format!("    implementation(\"{}\")\n", d));
                }
            }
            if lines.is_empty() {
                return body.to_string();
            }
            let mut out = String::with_capacity(body.len() + lines.len());
            out.push_str(&body[..end]);
            out.push_str(&lines);
            out.push_str(&body[end..]);
            return out;
        }
    }
    let mut out = body.to_string();
    out.push_str("\n\ndependencies {\n");
    for d in deps {
        out.push_str(&format!("    implementation(\"{}\")\n", d));
    }
    out.push_str("}\n");
    out
}

/// Inject deep-link intent filters + singleTop launch mode into AndroidManifest.xml.
/// All three mutations are scoped to the existing
/// `<activity android:name=".PerryActivity">` block in the template.
/// The function is best-effort: package.json missing or no deepLinks
/// section → no-op.
pub fn inject_android_deeplinks(
    manifest_path: &Path,
    project_root: &Path,
    format: OutputFormat,
) -> Result<()> {
    // Walk up from project_root to find package.json.
    let mut dir: PathBuf = project_root.to_path_buf();
    let mut deeplinks: Option<serde_json::Value> = None;
    for _ in 0..6 {
        let pkg = dir.join("package.json");
        if pkg.exists() {
            let data = std::fs::read_to_string(&pkg)?;
            let pkg_val: serde_json::Value = serde_json::from_str(&data)?;
            if let Some(dl) = pkg_val.get("perry").and_then(|p| p.get("deepLinks")) {
                deeplinks = Some(dl.clone());
            }
            break;
        }
        if !dir.pop() {
            break;
        }
    }
    let deeplinks = match deeplinks {
        Some(d) => d,
        None => return Ok(()), // No deepLinks section — leave manifest alone.
    };

    let mut manifest = std::fs::read_to_string(manifest_path)?;

    // Build the intent-filter blocks.
    let mut intent_filters = String::new();

    // Universal Links → App Links intent filter (autoVerify="true",
    // android:scheme="https", android:host="...", BROWSABLE category).
    if let Some(hosts) = deeplinks
        .get("universalLinks")
        .and_then(|u| u.get("android"))
        .and_then(|v| v.as_array())
    {
        for h in hosts {
            if let Some(host) = h.as_str() {
                intent_filters.push_str(&format!(
                    "            <intent-filter android:autoVerify=\"true\">\n                <action android:name=\"android.intent.action.VIEW\" />\n                <category android:name=\"android.intent.category.DEFAULT\" />\n                <category android:name=\"android.intent.category.BROWSABLE\" />\n                <data android:scheme=\"https\" android:host=\"{host}\" />\n            </intent-filter>\n",
                    host = host
                ));
            }
        }
    }

    // Custom schemes — `myapp://…` intent filter.
    if let Some(schemes) = deeplinks.get("schemes").and_then(|s| s.as_array()) {
        for s in schemes {
            if let Some(scheme) = s.as_str() {
                intent_filters.push_str(&format!(
                    "            <intent-filter>\n                <action android:name=\"android.intent.action.VIEW\" />\n                <category android:name=\"android.intent.category.DEFAULT\" />\n                <category android:name=\"android.intent.category.BROWSABLE\" />\n                <data android:scheme=\"{scheme}\" />\n            </intent-filter>\n",
                    scheme = scheme
                ));
            }
        }
    }

    if intent_filters.is_empty() {
        return Ok(());
    }

    // Locate the existing PerryActivity's <intent-filter> for
    // android.intent.action.MAIN — we insert the deep-link filters
    // immediately AFTER that block, still inside <activity>.
    // #1528: the template used to declare the activity as
    // `android:name=".PerryActivity"` but we now use the fully-
    // qualified Kotlin path so removing the manifest `package=`
    // doesn't break resolution. Match the trailing portion either way
    // so older / forked templates still work.
    let activity_pos = manifest
        .find("android:name=\"com.perry.app.PerryActivity\"")
        .or_else(|| manifest.find("android:name=\".PerryActivity\""))
        .ok_or_else(|| anyhow!("PerryActivity tag not found in AndroidManifest.xml"))?;

    // Add launchMode=singleTop to the activity tag if not already present.
    // The template has android:configChanges right after android:exported,
    // which is a safe insertion point.
    if !manifest[activity_pos..]
        .lines()
        .take(8)
        .any(|l| l.contains("android:launchMode"))
    {
        manifest = manifest.replacen(
            "android:configChanges=",
            "android:launchMode=\"singleTop\"\n            android:configChanges=",
            1,
        );
    }

    // Re-find the MAIN intent-filter close tag after the launchMode edit.
    let main_close = manifest
        .find("</intent-filter>")
        .ok_or_else(|| anyhow!("PerryActivity MAIN intent-filter not found"))?;
    let insert_at = main_close + "</intent-filter>".len();
    manifest.insert_str(insert_at, &format!("\n{}", intent_filters.trim_end()));

    std::fs::write(manifest_path, &manifest)?;

    if let OutputFormat::Text = format {
        let scheme_count = deeplinks
            .get("schemes")
            .and_then(|s| s.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let host_count = deeplinks
            .get("universalLinks")
            .and_then(|u| u.get("android"))
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        println!(
            "  Deep links: {} scheme(s) + {} App Link host(s) → AndroidManifest.xml",
            scheme_count, host_count
        );
    }
    Ok(())
}

/// Sign an unsigned APK with the Android debug keystore for local testing.
/// Creates the debug keystore if it doesn't exist.
pub fn debug_sign_apk(apk_path: &Path, format: OutputFormat) -> Result<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let debug_keystore = PathBuf::from(&home).join(".android/debug.keystore");

    // Create debug keystore if it doesn't exist
    if !debug_keystore.exists() {
        if let Some(parent) = debug_keystore.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let status = Command::new("keytool")
            .args([
                "-genkeypair",
                "-v",
                "-keystore",
                &debug_keystore.to_string_lossy(),
                "-storepass",
                "android",
                "-alias",
                "androiddebugkey",
                "-keypass",
                "android",
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "10000",
                "-dname",
                "CN=Android Debug,O=Android,C=US",
            ])
            .status()
            .map_err(|e| anyhow!("keytool not found: {}", e))?;
        if !status.success() {
            bail!("Failed to create debug keystore");
        }
    }

    if let OutputFormat::Text = format {
        println!("Signing APK with debug key...");
    }

    // Find apksigner from the Android SDK
    let android_home = std::env::var("ANDROID_HOME")
        .or_else(|_| std::env::var("ANDROID_SDK_ROOT"))
        .unwrap_or_else(|_| format!("{}/Library/Android/sdk", home));

    let apksigner = find_apksigner(&android_home);

    // zipalign first (required before signing)
    let aligned_path = apk_path.with_extension("aligned.apk");
    let zipalign = PathBuf::from(&android_home).join("build-tools");
    if let Some(zipalign_bin) = find_latest_build_tool(&zipalign, "zipalign") {
        let status = Command::new(&zipalign_bin)
            .args(["4"])
            .arg(apk_path)
            .arg(&aligned_path)
            .status();
        if let Ok(s) = status {
            if s.success() {
                std::fs::rename(&aligned_path, apk_path).ok();
            }
        }
    }

    // Sign with apksigner
    if let Some(signer) = apksigner {
        let status = Command::new(&signer)
            .args([
                "sign",
                "--ks",
                &debug_keystore.to_string_lossy(),
                "--ks-pass",
                "pass:android",
                "--ks-key-alias",
                "androiddebugkey",
                "--key-pass",
                "pass:android",
            ])
            .arg(apk_path)
            .status()
            .map_err(|e| anyhow!("apksigner failed: {}", e))?;
        if !status.success() {
            bail!("Failed to sign APK with debug keystore");
        }
    } else {
        // Fallback: use jarsigner
        let status = Command::new("jarsigner")
            .args([
                "-keystore",
                &debug_keystore.to_string_lossy(),
                "-storepass",
                "android",
                "-keypass",
                "android",
                "-signedjar",
            ])
            .arg(apk_path)
            .arg(apk_path)
            .arg("androiddebugkey")
            .status()
            .map_err(|e| anyhow!("jarsigner not found: {}", e))?;
        if !status.success() {
            bail!("Failed to sign APK with debug keystore");
        }
    }

    Ok(apk_path.to_path_buf())
}

/// Find apksigner in the Android SDK build-tools
pub fn find_apksigner(android_home: &str) -> Option<PathBuf> {
    find_latest_build_tool(
        &PathBuf::from(android_home).join("build-tools"),
        "apksigner",
    )
}

/// Find the latest version of a build tool
pub fn find_latest_build_tool(build_tools_dir: &Path, tool_name: &str) -> Option<PathBuf> {
    let mut versions: Vec<_> = std::fs::read_dir(build_tools_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    for v in versions {
        let tool = v.path().join(tool_name);
        if tool.exists() {
            return Some(tool);
        }
    }
    None
}

/// Install and launch an APK on an Android device/emulator via adb
pub fn install_and_launch_android(
    apk_path: &Path,
    bundle_id: &str,
    serial: &str,
    format: OutputFormat,
) -> Result<()> {
    // Debug-sign the APK if unsigned (Android requires signatures for install)
    debug_sign_apk(apk_path, format)?;

    if let OutputFormat::Text = format {
        println!();
        println!("Installing on {}...", serial);
    }

    let install = Command::new("adb")
        .args(["-s", serial, "install", "-r"])
        .arg(apk_path)
        .output()
        .map_err(|e| anyhow!("Failed to run adb install: {}", e))?;

    if !install.status.success() {
        let stderr = String::from_utf8_lossy(&install.stderr);
        let stdout = String::from_utf8_lossy(&install.stdout);
        return Err(anyhow!(
            "Failed to install APK on device {}: {}{}",
            serial,
            stderr,
            stdout
        ));
    }

    if let OutputFormat::Text = format {
        println!("Installed successfully.");
    }

    if let OutputFormat::Text = format {
        println!("Launching {}...", bundle_id);
        println!();
    }

    // Android activity name: use .PerryActivity (from the perry-ui-android template)
    let component = format!("{}/com.perry.app.PerryActivity", bundle_id);

    let launch = Command::new("adb")
        .args(["-s", serial, "shell", "am", "start", "-n", &component])
        .status()
        .map_err(|e| anyhow!("Failed to run adb shell am start: {}", e))?;

    if !launch.success() {
        return Err(anyhow!("Failed to launch app on device {}", serial));
    }

    if let OutputFormat::Text = format {
        println!("App launched. Streaming logs (Ctrl+C to stop)...");
        println!();
    }

    // Stream logcat filtered to the app's package
    // Use logcat's --pid if we can find the PID, otherwise fall back to grep
    std::thread::sleep(std::time::Duration::from_millis(1000));
    let pid = get_android_pid(serial, bundle_id);

    if !pid.is_empty() && pid != "0" {
        let _ = Command::new("adb")
            .args(["-s", serial, "logcat", "--pid", &pid])
            .status();
    } else {
        // Fallback: clear logcat and show all (app may not have started yet)
        let _ = Command::new("adb")
            .args(["-s", serial, "logcat", "-c"])
            .status();
        let _ = Command::new("adb").args(["-s", serial, "logcat"]).status();
    }

    Ok(())
}

/// Get the PID of a running Android app
pub fn get_android_pid(serial: &str, bundle_id: &str) -> String {
    // Try a few times since the app may still be starting
    for _ in 0..3 {
        let output = Command::new("adb")
            .args(["-s", serial, "shell", "pidof", "-s", bundle_id])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let pid = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !pid.is_empty() {
                    return pid;
                }
            }
            _ => {}
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `apply_wear_overlay` must transform a copy of the *real* Android template
    /// into a Wear OS project: watch feature + standalone meta-data in the
    /// manifest, and `androidx.wear` + `minSdk = 30` in the Gradle build. This
    /// runs against the checked-in template so anchor drift (a renamed tag or a
    /// changed minSdk) fails here instead of silently producing a phone APK.
    #[test]
    fn wear_overlay_applies_to_real_template() {
        let template = Path::new(env!("CARGO_MANIFEST_DIR")).join("../perry-ui-android/template");
        let manifest_src = template.join("app/src/main/AndroidManifest.xml");
        let gradle_src = template.join("app/build.gradle.kts");
        assert!(
            manifest_src.exists() && gradle_src.exists(),
            "android template not found at {}",
            template.display()
        );

        // Materialize a throwaway build dir with just the two files the overlay
        // touches, mirroring the layout build_and_run_android_impl produces.
        let build_dir =
            std::env::temp_dir().join(format!("perry_wear_overlay_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&build_dir);
        let app_main = build_dir.join("app/src/main");
        std::fs::create_dir_all(&app_main).unwrap();
        std::fs::copy(&manifest_src, app_main.join("AndroidManifest.xml")).unwrap();
        std::fs::copy(&gradle_src, build_dir.join("app/build.gradle.kts")).unwrap();

        apply_wear_overlay(&build_dir, OutputFormat::Json).unwrap();

        let manifest = std::fs::read_to_string(app_main.join("AndroidManifest.xml")).unwrap();
        let gradle = std::fs::read_to_string(build_dir.join("app/build.gradle.kts")).unwrap();

        assert!(
            manifest.contains("android.hardware.type.watch"),
            "watch uses-feature not injected"
        );
        assert!(
            manifest.contains("com.google.android.wearable.standalone"),
            "standalone meta-data not injected"
        );
        assert!(
            gradle.contains("androidx.wear:wear"),
            "androidx.wear dependency not injected"
        );
        assert!(
            gradle.contains("minSdk = 30") && !gradle.contains("minSdk = 24"),
            "minSdk not raised to 30 for Wear OS"
        );

        let _ = std::fs::remove_dir_all(&build_dir);
    }

    /// The overlay must be idempotent — running it twice (e.g. a rebuild into a
    /// reused dir) must not double-inject the watch feature or wear deps.
    #[test]
    fn wear_overlay_is_idempotent() {
        let template = Path::new(env!("CARGO_MANIFEST_DIR")).join("../perry-ui-android/template");
        let build_dir =
            std::env::temp_dir().join(format!("perry_wear_overlay_idem_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&build_dir);
        let app_main = build_dir.join("app/src/main");
        std::fs::create_dir_all(&app_main).unwrap();
        std::fs::copy(
            template.join("app/src/main/AndroidManifest.xml"),
            app_main.join("AndroidManifest.xml"),
        )
        .unwrap();
        std::fs::copy(
            template.join("app/build.gradle.kts"),
            build_dir.join("app/build.gradle.kts"),
        )
        .unwrap();

        apply_wear_overlay(&build_dir, OutputFormat::Json).unwrap();
        apply_wear_overlay(&build_dir, OutputFormat::Json).unwrap();

        let manifest = std::fs::read_to_string(app_main.join("AndroidManifest.xml")).unwrap();
        let gradle = std::fs::read_to_string(build_dir.join("app/build.gradle.kts")).unwrap();
        assert_eq!(
            manifest.matches("android.hardware.type.watch").count(),
            1,
            "watch feature injected more than once"
        );
        assert_eq!(
            gradle.matches("androidx.wear:wear").count(),
            1,
            "androidx.wear dependency injected more than once"
        );

        let _ = std::fs::remove_dir_all(&build_dir);
    }

    /// #867 — `inject_ads_android_manifest` must splice the
    /// GoogleMobileAds `APPLICATION_ID` meta-data into a copy of the real
    /// Android template manifest, reading the value from `[ads]` in
    /// perry.toml. Running it twice must not double-inject.
    #[test]
    fn ads_manifest_injects_application_id_and_is_idempotent() {
        let template = Path::new(env!("CARGO_MANIFEST_DIR")).join("../perry-ui-android/template");
        let manifest_src = template.join("app/src/main/AndroidManifest.xml");
        assert!(
            manifest_src.exists(),
            "android template not found at {}",
            template.display()
        );

        // project_root holds perry.toml; build_dir mirrors the Gradle
        // layout the real build produces.
        let root =
            std::env::temp_dir().join(format!("perry_ads_manifest_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let build_dir = root.join("build");
        let app_main = build_dir.join("app/src/main");
        std::fs::create_dir_all(&app_main).unwrap();
        std::fs::copy(&manifest_src, app_main.join("AndroidManifest.xml")).unwrap();
        std::fs::write(
            root.join("perry.toml"),
            "[ads]\nandroid_app_id = \"ca-app-pub-3940256099942544~3347511713\"\n",
        )
        .unwrap();

        inject_ads_android_manifest(&build_dir, &root, OutputFormat::Json).unwrap();
        // Second run must be a no-op (idempotency guard).
        inject_ads_android_manifest(&build_dir, &root, OutputFormat::Json).unwrap();

        let manifest = std::fs::read_to_string(app_main.join("AndroidManifest.xml")).unwrap();
        assert!(
            manifest.contains("com.google.android.gms.ads.APPLICATION_ID"),
            "APPLICATION_ID meta-data not injected"
        );
        assert!(manifest.contains("ca-app-pub-3940256099942544~3347511713"));
        assert_eq!(
            manifest
                .matches("com.google.android.gms.ads.APPLICATION_ID")
                .count(),
            1,
            "APPLICATION_ID injected more than once"
        );
        // Spliced inside <application>, before its close tag.
        let app_id_pos = manifest.find("APPLICATION_ID").unwrap();
        let close_pos = manifest.find("</application>").unwrap();
        assert!(app_id_pos < close_pos, "meta-data not inside <application>");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// No `[ads]` block (or no `android_app_id`) → manifest untouched.
    #[test]
    fn ads_manifest_no_op_when_unconfigured() {
        let template = Path::new(env!("CARGO_MANIFEST_DIR")).join("../perry-ui-android/template");
        let root =
            std::env::temp_dir().join(format!("perry_ads_manifest_noop_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let build_dir = root.join("build");
        let app_main = build_dir.join("app/src/main");
        std::fs::create_dir_all(&app_main).unwrap();
        std::fs::copy(
            template.join("app/src/main/AndroidManifest.xml"),
            app_main.join("AndroidManifest.xml"),
        )
        .unwrap();
        // perry.toml with an unrelated section only.
        std::fs::write(root.join("perry.toml"), "[ios]\nbundle_id = \"a\"\n").unwrap();

        inject_ads_android_manifest(&build_dir, &root, OutputFormat::Json).unwrap();

        let manifest = std::fs::read_to_string(app_main.join("AndroidManifest.xml")).unwrap();
        assert!(!manifest.contains("com.google.android.gms.ads.APPLICATION_ID"));

        let _ = std::fs::remove_dir_all(&root);
    }
}
