//! Apple-platform Info.plist mutators extracted from `compile.rs`.
//!
//! Two host-side post-codegen plist patches:
//!
//! - `inject_ios_deeplinks` (#583) — read `package.json` `perry.deepLinks` and
//!   inject CFBundleURLTypes into the generated Info.plist, plus write an
//!   `app.entitlements` for any `applinks:` associated domains.
//! - `inject_google_auth_info_plist` (#674/#1138) — read `[google_auth]` from
//!   `perry.toml` and inject `GIDClientID` / `GIDServerClientID` /
//!   `GIDDefaultScopes` for the `@perryts/google-auth` Swift bridge.
//! - `inject_ads_info_plist` (#867) — read `[ads]` from `perry.toml` and inject
//!   `GADApplicationIdentifier` (+ `NSUserTrackingUsageDescription`) for the
//!   Google Mobile Ads SDK.
//!
//! Both functions follow the same shape: return the mutated plist on success,
//! `None` on any read/parse/write failure. The orchestrator falls back to the
//! unmutated plist on `None` — matches the existing config-helper convention.

use std::fs;

use crate::OutputFormat;

/// Issue #583 — read `package.json` `perry.deepLinks` and inject the
/// generated CFBundleURLTypes into `info_plist`, plus write an
/// `app.entitlements` file alongside the bundle for any `applinks:`
/// associated domains. Returns the mutated plist on success, `None` on
/// any read/parse/write failure (caller falls back to the unmutated
/// plist — matches existing config-helper convention in this file).
pub(super) fn inject_ios_deeplinks(
    info_plist: &str,
    input: &std::path::Path,
    app_dir: &std::path::Path,
    format: OutputFormat,
) -> Option<String> {
    let mut dir = input.canonicalize().ok()?;
    let mut deeplinks: Option<serde_json::Value> = None;
    for _ in 0..5 {
        dir = dir.parent()?.to_path_buf();
        let pkg = dir.join("package.json");
        if pkg.exists() {
            let data = fs::read_to_string(&pkg).ok()?;
            let pkg_val: serde_json::Value = serde_json::from_str(&data).ok()?;
            if let Some(dl) = pkg_val.get("perry").and_then(|p| p.get("deepLinks")) {
                deeplinks = Some(dl.clone());
            }
            break;
        }
    }
    let deeplinks = deeplinks?;

    let bundle_id_for_url_name = lookup_bundle_id_from_info_plist(info_plist)
        .unwrap_or_else(|| "perry.deeplink".to_string());

    // CFBundleURLTypes — one entry per scheme.
    let mut url_types_xml = String::new();
    if let Some(schemes) = deeplinks
        .get("schemes")
        .and_then(|s| s.as_array())
        .filter(|a| !a.is_empty())
    {
        url_types_xml.push_str("    <key>CFBundleURLTypes</key>\n    <array>\n");
        for scheme in schemes {
            if let Some(s) = scheme.as_str() {
                url_types_xml.push_str(&format!(
                    "        <dict>\n            <key>CFBundleURLName</key>\n            <string>{bundle}.{name}</string>\n            <key>CFBundleURLSchemes</key>\n            <array>\n                <string>{name}</string>\n            </array>\n        </dict>\n",
                    bundle = bundle_id_for_url_name,
                    name = s
                ));
            }
        }
        url_types_xml.push_str("    </array>\n");
    }

    // Associated domains entitlement — written to a sidecar
    // `app.entitlements` file; the user's signing pipeline picks it up.
    let universal_hosts: Vec<String> = deeplinks
        .get("universalLinks")
        .and_then(|u| u.get("ios"))
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|h| h.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if !universal_hosts.is_empty() {
        let mut entitlements = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n    <key>com.apple.developer.associated-domains</key>\n    <array>\n",
        );
        for host in &universal_hosts {
            entitlements.push_str(&format!("        <string>applinks:{}</string>\n", host));
        }
        entitlements.push_str("    </array>\n</dict>\n</plist>\n");
        let entitlements_path = app_dir.join("app.entitlements");
        fs::write(&entitlements_path, entitlements).ok()?;
        if let OutputFormat::Text = format {
            println!(
                "  Deep links: {} associated domain(s) → {}",
                universal_hosts.len(),
                entitlements_path.display()
            );
            println!(
                "  Sign with: codesign --entitlements {} ...",
                entitlements_path.display()
            );
        }
    }

    if url_types_xml.is_empty() {
        // Nothing to inject (only universal links configured) — return
        // the unmutated plist; the entitlements file is written either way.
        return Some(info_plist.to_string());
    }
    if let OutputFormat::Text = format {
        let scheme_count = deeplinks
            .get("schemes")
            .and_then(|s| s.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        println!(
            "  Deep links: {} URL scheme(s) → CFBundleURLTypes",
            scheme_count
        );
    }
    Some(info_plist.replace(
        "</dict>\n</plist>",
        &format!("{}</dict>\n</plist>", url_types_xml),
    ))
}

/// Issue #1138 — read `[google_auth]` from `perry.toml` and inject the
/// GoogleSignIn-required keys (`GIDClientID`, `GIDServerClientID`,
/// `GIDDefaultScopes`) into `info_plist`. The Swift bridge in
/// `@perryts/google-auth` reads them via `Bundle.main.infoDictionary`.
///
/// Returns the mutated plist on success, `None` if the block isn't
/// present / the read failed. The OAuth-redirect URL scheme
/// (`com.googleusercontent.apps.<reversed-id>`) must currently be
/// added by the user through `perry.deepLinks.schemes` in
/// `package.json` or `[ios.info_plist]` in `perry.toml`; auto-emission
/// is a follow-up that needs to merge into the existing
/// `CFBundleURLTypes` array without duplicating the key.
pub(super) fn inject_google_auth_info_plist(
    info_plist: &str,
    input: &std::path::Path,
    format: OutputFormat,
) -> Option<String> {
    let mut dir = input.canonicalize().ok()?;
    let mut data: Option<String> = None;
    for _ in 0..5 {
        dir = dir.parent()?.to_path_buf();
        let toml_path = dir.join("perry.toml");
        if toml_path.exists() {
            data = fs::read_to_string(&toml_path).ok();
            break;
        }
    }
    let raw = data?;
    let doc: toml::Table = raw.parse().ok()?;
    let ga = doc.get("google_auth")?.as_table()?;

    let mut entries = String::new();
    if let Some(client) = ga.get("ios_client_id").and_then(|v| v.as_str()) {
        entries.push_str(&format!(
            "    <key>GIDClientID</key>\n    <string>{}</string>\n",
            client
        ));
    }
    if let Some(server) = ga.get("server_client_id").and_then(|v| v.as_str()) {
        entries.push_str(&format!(
            "    <key>GIDServerClientID</key>\n    <string>{}</string>\n",
            server
        ));
    }
    if let Some(scopes) = ga.get("default_scopes").and_then(|v| v.as_array()) {
        let mut arr = String::from("    <key>GIDDefaultScopes</key>\n    <array>\n");
        for s in scopes {
            if let Some(scope) = s.as_str() {
                arr.push_str(&format!("        <string>{}</string>\n", scope));
            }
        }
        arr.push_str("    </array>\n");
        entries.push_str(&arr);
    }

    if entries.is_empty() {
        return None;
    }

    if let OutputFormat::Text = format {
        println!("  google_auth: injected GoogleSignIn Info.plist keys");
    }

    Some(info_plist.replace(
        "</dict>\n</plist>",
        &format!("{}</dict>\n</plist>", entries),
    ))
}

/// #867 — read `[ads]` from `perry.toml` and inject the Google Mobile
/// Ads keys into `info_plist`:
///
/// - `GADApplicationIdentifier` (from `ios_app_id`) — required; the SDK
///   raises an exception at `start()` if it's missing.
/// - `NSUserTrackingUsageDescription` (from `att_usage_description`) —
///   the purpose string iOS shows in the App Tracking Transparency
///   prompt. Only emitted when configured, and only if the app hasn't
///   already declared it via `[ios.info_plist]`.
///
/// Returns the mutated plist on success, `None` if the block / the
/// `ios_app_id` key is absent (caller falls back to the unmutated
/// plist). Mirrors `inject_google_auth_info_plist`.
pub(super) fn inject_ads_info_plist(
    info_plist: &str,
    input: &std::path::Path,
    format: OutputFormat,
) -> Option<String> {
    let mut dir = input.canonicalize().ok()?;
    let mut data: Option<String> = None;
    for _ in 0..5 {
        dir = dir.parent()?.to_path_buf();
        let toml_path = dir.join("perry.toml");
        if toml_path.exists() {
            data = fs::read_to_string(&toml_path).ok();
            break;
        }
    }
    let raw = data?;
    let doc: toml::Table = raw.parse().ok()?;
    let ads = doc.get("ads")?.as_table()?;

    // GADApplicationIdentifier is the only required key — without it the
    // SDK can't start, so a `[ads]` block with no `ios_app_id` is a no-op
    // for the Apple side.
    let app_id = ads.get("ios_app_id").and_then(|v| v.as_str())?;

    // App IDs are `ca-app-pub-<digits>~<digits>` (no XML specials), but
    // escape defensively so a malformed value can't produce an invalid
    // plist — matches the Android-manifest injector.
    let app_id_escaped = app_id
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let mut entries = String::new();
    entries.push_str(&format!(
        "    <key>GADApplicationIdentifier</key>\n    <string>{}</string>\n",
        app_id_escaped
    ));

    // ATT purpose string is free-form user text — escape XML specials.
    // Skip it if the app already declared NSUserTrackingUsageDescription
    // (e.g. via [ios.info_plist]) so we don't emit a duplicate key.
    if let Some(desc) = ads.get("att_usage_description").and_then(|v| v.as_str()) {
        if !info_plist.contains("NSUserTrackingUsageDescription") {
            let escaped = desc
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            entries.push_str(&format!(
                "    <key>NSUserTrackingUsageDescription</key>\n    <string>{}</string>\n",
                escaped
            ));
        }
    }

    if let OutputFormat::Text = format {
        println!("  ads: injected GoogleMobileAds Info.plist keys (#867)");
    }

    Some(info_plist.replace(
        "</dict>\n</plist>",
        &format!("{}</dict>\n</plist>", entries),
    ))
}

/// #1178 / #675 — write (or augment) `app.entitlements` with the
/// `com.apple.security.application-groups` array when `app_group` is
/// configured in perry.toml (`[ios]` / `[watchos]` — the entitlement XML
/// is identical across Apple platforms, so both bundlers share this).
///
/// Idempotent with `inject_ios_deeplinks`: if the deeplinks pass
/// already wrote an entitlements file (e.g. associated-domains for
/// `applinks:`), we splice our `<key>...</key><array>...</array>`
/// before the closing `</dict>` instead of clobbering it. Otherwise
/// we emit the full plist wrapper. Either way the signing pipeline
/// (iOS resign / watchOS `codesign_apple_bundle`) picks up a single
/// `app.entitlements` file at codesign time.
pub(super) fn inject_app_group_entitlement(
    app_dir: &std::path::Path,
    app_group: Option<&str>,
    format: OutputFormat,
) -> Option<()> {
    let app_group = app_group.filter(|s| !s.is_empty())?;
    let entitlements_path = app_dir.join("app.entitlements");

    let key_block = format!(
        "    <key>com.apple.security.application-groups</key>\n    <array>\n        <string>{}</string>\n    </array>\n",
        app_group
    );

    let new_contents = match fs::read_to_string(&entitlements_path) {
        Ok(existing) => {
            // Already declared? leave the user's hand-written or
            // deeplinks-injected entry alone.
            if existing.contains("com.apple.security.application-groups") {
                return Some(());
            }
            existing.replace(
                "</dict>\n</plist>",
                &format!("{}</dict>\n</plist>", key_block),
            )
        }
        Err(_) => {
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n{}</dict>\n</plist>\n",
                key_block
            )
        }
    };

    fs::write(&entitlements_path, new_contents).ok()?;

    if let OutputFormat::Text = format {
        println!(
            "  App Group: {} → {} (#1178)",
            app_group,
            entitlements_path.display()
        );
        println!(
            "  Sign with: codesign --entitlements {} ...",
            entitlements_path.display()
        );
    }
    Some(())
}

/// #5074 — write (or augment) `app.entitlements` with the `aps-environment`
/// entitlement when `[ios] push_notifications = true` is set in perry.toml.
/// Without this entitlement `[UIApplication registerForRemoteNotifications]`
/// (`notificationRegisterRemote` in `perry/system`) always fails and no APNs
/// token is ever produced.
///
/// The value defaults to `development` (matching the dev-signed bundles
/// `perry compile --target ios` produces); an explicit
/// `[ios] push_environment = "production"` overrides it for distribution
/// builds. Any value other than `production` (including a typo) resolves to
/// `development`, the safe default for on-device debugging.
///
/// Idempotent with `inject_ios_deeplinks` / `inject_app_group_entitlement`:
/// if an entitlements file already exists we splice our `<key>...</key>` before
/// the closing `</dict>` (and leave any hand-written `aps-environment` alone);
/// otherwise we emit the full plist wrapper. Either way the user's signing
/// pipeline picks up a single `app.entitlements` at codesign time, and the
/// dev-resign path (`build_dev_entitlements_xml`) layers its development keys
/// on top without dropping this one.
pub(super) fn inject_ios_push_entitlement(
    input: &std::path::Path,
    app_dir: &std::path::Path,
    format: OutputFormat,
) -> Option<()> {
    let (enabled, environment) = read_ios_push_config(input)?;
    if !enabled {
        return None;
    }

    let entitlements_path = app_dir.join("app.entitlements");
    let key_block = format!(
        "    <key>aps-environment</key>\n    <string>{}</string>\n",
        environment
    );

    let new_contents = match fs::read_to_string(&entitlements_path) {
        Ok(existing) => {
            // Already declared (hand-written or a previous run)? leave it alone.
            if existing.contains("aps-environment") {
                return Some(());
            }
            existing.replace(
                "</dict>\n</plist>",
                &format!("{}</dict>\n</plist>", key_block),
            )
        }
        Err(_) => format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n{}</dict>\n</plist>\n",
            key_block
        ),
    };

    fs::write(&entitlements_path, new_contents).ok()?;

    if let OutputFormat::Text = format {
        println!(
            "  Push notifications: aps-environment={} → {} (#5074)",
            environment,
            entitlements_path.display()
        );
        println!(
            "  Sign with: codesign --entitlements {} ...",
            entitlements_path.display()
        );
    }
    Some(())
}

/// Resolve `[ios] push_notifications` (bool) and `[ios] push_environment`
/// (string) from the nearest `perry.toml` walking up from `input`. Returns
/// `(enabled, environment)` where `environment` is normalized to
/// `"development"` unless explicitly set to `"production"`. `None` on any
/// missing-file / parse failure (caller skips injection — matches the
/// config-helper convention in this file).
fn read_ios_push_config(input: &std::path::Path) -> Option<(bool, String)> {
    let mut dir = input.canonicalize().ok()?;
    let mut data: Option<String> = None;
    for _ in 0..5 {
        dir = dir.parent()?.to_path_buf();
        let toml_path = dir.join("perry.toml");
        if toml_path.exists() {
            data = fs::read_to_string(&toml_path).ok();
            break;
        }
    }
    let doc: toml::Table = data?.parse().ok()?;
    Some(parse_ios_push_config(&doc))
}

/// Pure resolver shared by `read_ios_push_config` and the unit tests.
/// `[ios] push_notifications` opts in; `[ios] push_environment` selects the
/// APNs environment (`production`, else `development`).
fn parse_ios_push_config(doc: &toml::Table) -> (bool, String) {
    let ios = doc.get("ios").and_then(|v| v.as_table());
    let enabled = ios
        .and_then(|t| t.get("push_notifications"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let environment = ios
        .and_then(|t| t.get("push_environment"))
        .and_then(|v| v.as_str())
        .filter(|s| *s == "production")
        .unwrap_or("development")
        .to_string();
    (enabled, environment)
}

/// Cheap CFBundleIdentifier extraction from an in-memory Info.plist string.
/// We need it for the CFBundleURLName field (Apple's convention is
/// `<bundle-id>.<scheme>`). Falls back to `perry.deeplink` when the
/// expected `<string>...</string>` shape isn't found.
fn lookup_bundle_id_from_info_plist(info_plist: &str) -> Option<String> {
    let key = "<key>CFBundleIdentifier</key>";
    let after_key = info_plist.find(key)? + key.len();
    let rest = &info_plist[after_key..];
    let start = rest.find("<string>")? + "<string>".len();
    let end = rest[start..].find("</string>")?;
    Some(rest[start..start + end].trim().to_string())
}

#[cfg(test)]
mod push_entitlement_tests {
    use super::{inject_ios_push_entitlement, parse_ios_push_config};
    use crate::OutputFormat;

    fn parse(src: &str) -> toml::Table {
        src.parse::<toml::Table>().unwrap()
    }

    #[test]
    fn push_config_defaults_to_development_when_opted_in() {
        // #5074 — `push_notifications = true` opts in; environment defaults to
        // `development` (the dev-signed bundles `perry compile --target ios`
        // produces).
        let (enabled, env) = parse_ios_push_config(&parse("[ios]\npush_notifications = true\n"));
        assert!(enabled);
        assert_eq!(env, "development");
    }

    #[test]
    fn push_config_honors_production_environment() {
        let (enabled, env) = parse_ios_push_config(&parse(
            "[ios]\npush_notifications = true\npush_environment = \"production\"\n",
        ));
        assert!(enabled);
        assert_eq!(env, "production");
    }

    #[test]
    fn push_config_clamps_unknown_environment_to_development() {
        let (enabled, env) = parse_ios_push_config(&parse(
            "[ios]\npush_notifications = true\npush_environment = \"sandbox\"\n",
        ));
        assert!(enabled);
        assert_eq!(env, "development");
    }

    #[test]
    fn push_config_off_when_absent_or_false() {
        assert!(!parse_ios_push_config(&parse("[ios]\nbundle_id = \"a\"\n")).0);
        assert!(!parse_ios_push_config(&parse("[ios]\npush_notifications = false\n")).0);
        assert!(!parse_ios_push_config(&parse("")).0);
    }

    #[test]
    fn injects_full_plist_when_no_entitlements_exist() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("src").join("main.ts");
        std::fs::create_dir_all(input.parent().unwrap()).unwrap();
        std::fs::write(&input, "console.log('x')").unwrap();
        std::fs::write(
            dir.path().join("perry.toml"),
            "[ios]\npush_notifications = true\n",
        )
        .unwrap();
        let app_dir = dir.path().join("out.app");
        std::fs::create_dir_all(&app_dir).unwrap();

        assert!(inject_ios_push_entitlement(&input, &app_dir, OutputFormat::Json).is_some());

        let ent = std::fs::read_to_string(app_dir.join("app.entitlements")).unwrap();
        assert!(ent.starts_with("<?xml"));
        assert!(ent.contains("<key>aps-environment</key>"));
        assert!(ent.contains("<string>development</string>"));
        assert_eq!(ent.matches("</dict>").count(), 1);
        assert_eq!(ent.matches("</plist>").count(), 1);
    }

    #[test]
    fn splices_into_existing_app_group_entitlements_without_clobbering() {
        // Idempotent with #1178: an existing app.entitlements (e.g. App Group)
        // gets the aps-environment key spliced in, both keys survive, and the
        // wrapper stays single.
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("main.ts");
        std::fs::write(&input, "console.log('x')").unwrap();
        std::fs::write(
            dir.path().join("perry.toml"),
            "[ios]\npush_notifications = true\npush_environment = \"production\"\n",
        )
        .unwrap();
        let app_dir = dir.path().join("out.app");
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::write(
            app_dir.join("app.entitlements"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n    \
             <key>com.apple.security.application-groups</key>\n    <array>\n        \
             <string>group.com.example.shared</string>\n    </array>\n</dict>\n</plist>\n",
        )
        .unwrap();

        assert!(inject_ios_push_entitlement(&input, &app_dir, OutputFormat::Json).is_some());

        let ent = std::fs::read_to_string(app_dir.join("app.entitlements")).unwrap();
        assert!(ent.contains("com.apple.security.application-groups"));
        assert!(ent.contains("group.com.example.shared"));
        assert!(ent.contains("<key>aps-environment</key>"));
        assert!(ent.contains("<string>production</string>"));
        assert_eq!(ent.matches("</dict>").count(), 1);
        assert_eq!(ent.matches("</plist>").count(), 1);
    }

    #[test]
    fn idempotent_when_aps_environment_already_present() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("main.ts");
        std::fs::write(&input, "console.log('x')").unwrap();
        std::fs::write(
            dir.path().join("perry.toml"),
            "[ios]\npush_notifications = true\n",
        )
        .unwrap();
        let app_dir = dir.path().join("out.app");
        std::fs::create_dir_all(&app_dir).unwrap();
        let hand_written = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n    <key>aps-environment</key>\n    <string>production</string>\n</dict>\n</plist>\n";
        std::fs::write(app_dir.join("app.entitlements"), hand_written).unwrap();

        assert!(inject_ios_push_entitlement(&input, &app_dir, OutputFormat::Json).is_some());

        // Hand-written value preserved (not downgraded to development).
        let ent = std::fs::read_to_string(app_dir.join("app.entitlements")).unwrap();
        assert_eq!(ent, hand_written);
    }

    #[test]
    fn skips_when_not_opted_in() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("main.ts");
        std::fs::write(&input, "console.log('x')").unwrap();
        std::fs::write(dir.path().join("perry.toml"), "[ios]\nbundle_id = \"a\"\n").unwrap();
        let app_dir = dir.path().join("out.app");
        std::fs::create_dir_all(&app_dir).unwrap();

        assert!(inject_ios_push_entitlement(&input, &app_dir, OutputFormat::Json).is_none());
        assert!(!app_dir.join("app.entitlements").exists());
    }
}

#[cfg(test)]
mod ads_info_plist_tests {
    use super::inject_ads_info_plist;
    use crate::OutputFormat;

    const BASE_PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
        <plist version=\"1.0\">\n<dict>\n    \
        <key>CFBundleIdentifier</key>\n    <string>com.example.app</string>\n</dict>\n</plist>";

    /// Stage a project tree with `perry.toml` one dir above `src/main.ts`
    /// and return `(tempdir, input_path)`. The injector walks up from the
    /// input via `parent()`, so the toml must sit above the input.
    fn staged(toml_body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("src").join("main.ts");
        std::fs::create_dir_all(input.parent().unwrap()).unwrap();
        std::fs::write(&input, "console.log('x')").unwrap();
        std::fs::write(dir.path().join("perry.toml"), toml_body).unwrap();
        (dir, input)
    }

    #[test]
    fn injects_gad_application_identifier() {
        let (_dir, input) =
            staged("[ads]\nios_app_id = \"ca-app-pub-3940256099942544~1458002511\"\n");
        let out = inject_ads_info_plist(BASE_PLIST, &input, OutputFormat::Json).unwrap();
        assert!(out.contains("<key>GADApplicationIdentifier</key>"));
        assert!(out.contains("ca-app-pub-3940256099942544~1458002511"));
        // Wrapper integrity — exactly one closing dict/plist, keys spliced
        // before them.
        assert_eq!(out.matches("</dict>").count(), 1);
        assert_eq!(out.matches("</plist>").count(), 1);
        // Untouched existing keys survive.
        assert!(out.contains("<key>CFBundleIdentifier</key>"));
    }

    #[test]
    fn injects_att_usage_description_when_configured() {
        let (_dir, input) = staged(
            "[ads]\nios_app_id = \"ca-app-pub-XXXX~YYYY\"\natt_usage_description = \"Ads & you\"\n",
        );
        let out = inject_ads_info_plist(BASE_PLIST, &input, OutputFormat::Json).unwrap();
        assert!(out.contains("<key>NSUserTrackingUsageDescription</key>"));
        // Free-text purpose string gets XML-escaped.
        assert!(out.contains("Ads &amp; you"));
    }

    #[test]
    fn skips_att_key_when_already_declared() {
        // The app already set NSUserTrackingUsageDescription via
        // [ios.info_plist]; we must not emit a duplicate key.
        let with_att = BASE_PLIST.replace(
            "</dict>",
            "    <key>NSUserTrackingUsageDescription</key>\n    <string>existing</string>\n</dict>",
        );
        let (_dir, input) = staged(
            "[ads]\nios_app_id = \"ca-app-pub-XXXX~YYYY\"\natt_usage_description = \"new\"\n",
        );
        let out = inject_ads_info_plist(&with_att, &input, OutputFormat::Json).unwrap();
        assert_eq!(out.matches("NSUserTrackingUsageDescription").count(), 1);
        assert!(out.contains("<string>existing</string>"));
        // GADApplicationIdentifier still added.
        assert!(out.contains("<key>GADApplicationIdentifier</key>"));
    }

    #[test]
    fn escapes_xml_specials_in_app_id() {
        // A malformed app_id with XML specials must not produce an invalid
        // plist (#867 / CodeRabbit).
        let (_dir, input) = staged("[ads]\nios_app_id = \"ca&app<pub>id\"\n");
        let out = inject_ads_info_plist(BASE_PLIST, &input, OutputFormat::Json).unwrap();
        assert!(out.contains("ca&amp;app&lt;pub&gt;id"));
        assert!(!out.contains("ca&app<pub>id"));
    }

    #[test]
    fn no_op_when_block_or_id_absent() {
        // No [ads] block at all.
        let (_d1, i1) = staged("[ios]\nbundle_id = \"a\"\n");
        assert!(inject_ads_info_plist(BASE_PLIST, &i1, OutputFormat::Json).is_none());
        // [ads] present but no ios_app_id (Android-only config).
        let (_d2, i2) = staged("[ads]\nandroid_app_id = \"ca-app-pub-XXXX~ZZZZ\"\n");
        assert!(inject_ads_info_plist(BASE_PLIST, &i2, OutputFormat::Json).is_none());
    }
}
