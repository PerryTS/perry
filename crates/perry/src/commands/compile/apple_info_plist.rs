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
