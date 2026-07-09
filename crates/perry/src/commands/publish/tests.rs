#![cfg(test)]

use super::*;
use std::fs;
use std::path::Path;

#[test]
fn test_perry_config_roundtrip() {
    let config = PerryConfig {
        license_key: Some("FREE-abc123".into()),
        server: Some("https://build.example.com".into()),
        default_target: Some("macos".into()),
        apple: Some(AppleSavedConfig {
            team_id: Some("ABC123DEF".into()),
            p8_key_path: Some("/Users/me/AuthKey_XXX.p8".into()),
            key_id: Some("XXX".into()),
            issuer_id: Some("abc-def-ghi".into()),
        }),
        ios: Some(IosSavedConfig {}),
        android: Some(AndroidSavedConfig {
            keystore_path: Some("/Users/me/release.keystore".into()),
            key_alias: Some("key0".into()),
            google_play_key_path: Some("/Users/me/play-sa.json".into()),
        }),
        ..Default::default()
    };

    let toml_str = toml::to_string_pretty(&config).unwrap();
    let parsed: PerryConfig = toml::from_str(&toml_str).unwrap();

    assert_eq!(parsed.license_key, config.license_key);
    assert_eq!(parsed.server, config.server);
    assert_eq!(parsed.default_target, config.default_target);
    assert_eq!(
        parsed.apple.as_ref().unwrap().team_id,
        config.apple.as_ref().unwrap().team_id
    );
    assert_eq!(
        parsed.android.as_ref().unwrap().google_play_key_path,
        config.android.as_ref().unwrap().google_play_key_path
    );
}

#[test]
fn test_perry_config_minimal() {
    let config = PerryConfig {
        license_key: Some("FREE-test".into()),
        ..Default::default()
    };
    let toml_str = toml::to_string_pretty(&config).unwrap();
    assert!(toml_str.contains("license_key"));
    assert!(
        !toml_str.contains("[apple]"),
        "empty sections should be omitted"
    );
    assert!(!toml_str.contains("[android]"));
}

#[test]
fn test_perry_config_parse_legacy_format() {
    // Old format was just license_key = "..." — should still parse
    let legacy = r#"license_key = "FREE-legacy-key""#;
    let config: PerryConfig = toml::from_str(legacy).unwrap();
    assert_eq!(config.license_key.as_deref(), Some("FREE-legacy-key"));
    assert!(config.apple.is_none());
    assert!(config.default_target.is_none());
}

#[test]
fn test_perry_config_no_passwords_in_toml() {
    let config = PerryConfig {
        license_key: Some("FREE-test".into()),
        android: Some(AndroidSavedConfig {
            keystore_path: Some("/path/to/keystore".into()),
            key_alias: Some("key0".into()),
            google_play_key_path: None,
        }),
        ..Default::default()
    };
    let toml_str = toml::to_string_pretty(&config).unwrap();
    // Config struct intentionally has no password fields
    assert!(!toml_str.contains("password"));
    assert!(toml_str.contains("keystore_path"));
}

#[test]
fn test_perry_config_default_is_empty() {
    let config = PerryConfig::default();
    assert!(config.license_key.is_none());
    assert!(config.server.is_none());
    assert!(config.default_target.is_none());
    assert!(config.apple.is_none());
    assert!(config.ios.is_none());
    assert!(config.android.is_none());
}

#[test]
fn test_credentials_payload_with_google_play() {
    let creds = CredentialsPayload {
        apple_team_id: None,
        apple_signing_identity: None,
        apple_key_id: None,
        apple_issuer_id: None,
        apple_p8_key: None,
        provisioning_profile_base64: None,
        apple_certificate_p12_base64: None,
        apple_certificate_password: None,
        apple_notarize_certificate_p12_base64: None,
        apple_notarize_certificate_password: None,
        apple_notarize_signing_identity: None,
        apple_installer_certificate_p12_base64: None,
        apple_installer_certificate_password: None,
        android_keystore_base64: Some("dGVzdA==".into()),
        android_keystore_password: Some("pass".into()),
        android_key_alias: Some("key0".into()),
        android_key_password: None,
        google_play_service_account_json: Some("{\"client_email\":\"test@gcp\"}".into()),
        gcloud_kms_key: None,
        gcloud_kms_cert_base64: None,
        gcloud_service_account_base64: None,
    };
    let json = serde_json::to_string(&creds).unwrap();
    assert!(json.contains("google_play_service_account_json"));
    assert!(json.contains("client_email"));
}

#[test]
fn test_credentials_payload_omits_none() {
    let creds = CredentialsPayload {
        apple_team_id: Some("ABC".into()),
        apple_signing_identity: Some("Dev ID".into()),
        apple_key_id: None,
        apple_issuer_id: None,
        apple_p8_key: None,
        provisioning_profile_base64: None,
        apple_certificate_p12_base64: None,
        apple_certificate_password: None,
        apple_notarize_certificate_p12_base64: None,
        apple_notarize_certificate_password: None,
        apple_notarize_signing_identity: None,
        apple_installer_certificate_p12_base64: None,
        apple_installer_certificate_password: None,
        android_keystore_base64: None,
        android_keystore_password: None,
        android_key_alias: None,
        android_key_password: None,
        google_play_service_account_json: None,
        gcloud_kms_key: None,
        gcloud_kms_cert_base64: None,
        gcloud_service_account_base64: None,
    };
    let json = serde_json::to_string(&creds).unwrap();
    // Fields with skip_serializing_if should be absent
    assert!(!json.contains("android_keystore_base64"));
    assert!(!json.contains("google_play_service_account_json"));
    // Non-skip fields are always present (even as null)
    assert!(json.contains("apple_team_id"));
}

#[test]
fn test_resolve_credential_cli_wins() {
    let result = resolve_credential(
        Some("from-cli"),
        "NONEXISTENT_ENV_VAR_XYZ",
        Some("from-saved"),
        "test",
        false,
        false, // not interactive
    );
    assert_eq!(result.as_deref(), Some("from-cli"));
}

#[test]
fn test_resolve_credential_saved_fallback() {
    let result = resolve_credential(
        None,
        "NONEXISTENT_ENV_VAR_XYZ",
        Some("from-saved"),
        "test",
        false,
        false,
    );
    assert_eq!(result.as_deref(), Some("from-saved"));
}

#[test]
fn test_resolve_credential_none_when_missing() {
    let result = resolve_credential(None, "NONEXISTENT_ENV_VAR_XYZ", None, "test", false, false);
    assert!(result.is_none());
}

#[test]
fn test_resolve_credential_skips_empty() {
    let result = resolve_credential(
        Some(""),
        "NONEXISTENT_ENV_VAR_XYZ",
        Some("saved"),
        "test",
        false,
        false,
    );
    assert_eq!(result.as_deref(), Some("saved"));
}

#[test]
fn test_resolve_path_credential() {
    let result = resolve_path_credential(
        Some(Path::new("/path/to/file")),
        "NONEXISTENT_ENV_VAR_XYZ",
        Some("/saved/path"),
        "test",
        false,
    );
    assert_eq!(result.as_deref(), Some("/path/to/file"));
}

#[test]
fn test_resolve_path_credential_saved_fallback() {
    let result = resolve_path_credential(
        None,
        "NONEXISTENT_ENV_VAR_XYZ",
        Some("/saved/path"),
        "test",
        false,
    );
    assert_eq!(result.as_deref(), Some("/saved/path"));
}

#[test]
fn test_config_file_write_and_read() {
    // Test writing to a temp location and reading back
    let dir = std::env::temp_dir().join("perry-test-config");
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("config.toml");

    let config = PerryConfig {
        license_key: Some("TEST-KEY-123".into()),
        default_target: Some("ios".into()),
        apple: Some(AppleSavedConfig {
            team_id: Some("TEAM123".into()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let content = toml::to_string_pretty(&config).unwrap();
    fs::write(&path, &content).unwrap();

    let read_back = fs::read_to_string(&path).unwrap();
    let parsed: PerryConfig = toml::from_str(&read_back).unwrap();
    assert_eq!(parsed.license_key.as_deref(), Some("TEST-KEY-123"));
    assert_eq!(parsed.default_target.as_deref(), Some("ios"));
    assert_eq!(parsed.apple.unwrap().team_id.as_deref(), Some("TEAM123"));

    // Cleanup
    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir(&dir);
}

// The trailing `false, None, false, None` on each call is
// (is_tvos, tvos_distribute, is_watchos, watchos_distribute) — not applicable
// to the android/ios/macos cases below. tvOS/watchOS have dedicated tests.

#[test]
fn test_validate_android_playstore_requires_json() {
    let result = validate_credentials_for_distribute(
        true,
        Some("playstore"),
        None, // android, no key
        false,
        None,
        None,
        None,
        None, // ios not applicable
        false,
        None, // macos not applicable
        false,
        None, // tvos not applicable
        false,
        None, // watchos not applicable
    );
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("service account JSON key"), "{msg}");
    assert!(msg.contains("perry setup android"), "{msg}");
}

#[test]
fn test_validate_android_playstore_invalid_track() {
    let result = validate_credentials_for_distribute(
        true,
        Some("playstore:bogus"),
        Some("{}"),
        false,
        None,
        None,
        None,
        None,
        false,
        None,
        false,
        None,
        false,
        None,
    );
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid Play Store track"));
}

#[test]
fn test_validate_android_playstore_valid_tracks() {
    for track in ["internal", "alpha", "beta", "production"] {
        let distribute = format!("playstore:{track}");
        let result = validate_credentials_for_distribute(
            true,
            Some(&distribute),
            Some("{\"ok\":1}"),
            false,
            None,
            None,
            None,
            None,
            false,
            None,
            false,
            None,
            false,
            None,
        );
        assert!(result.is_ok(), "track={track} should be valid");
    }
}

#[test]
fn test_validate_ios_appstore_requires_creds() {
    let result = validate_credentials_for_distribute(
        false,
        None,
        None,
        true,
        Some("appstore"),
        None,
        None,
        None, // ios, missing creds
        false,
        None,
        false,
        None,
        false,
        None,
    );
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("App Store Connect API credentials"), "{msg}");
    assert!(msg.contains("perry setup ios"), "{msg}");
}

#[test]
fn test_validate_ios_testflight_requires_creds() {
    let result = validate_credentials_for_distribute(
        false,
        None,
        None,
        true,
        Some("testflight"),
        Some("kid"),
        None,
        Some("key_content"),
        false,
        None,
        false,
        None,
        false,
        None,
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Issuer ID"));
}

#[test]
fn test_validate_ios_no_distribute_passes() {
    let result = validate_credentials_for_distribute(
        false, None, None, true, None, None, None, None, // ios but no distribute set
        false, None, false, None, false, None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_validate_macos_appstore_requires_creds() {
    let result = validate_credentials_for_distribute(
        false,
        None,
        None,
        false,
        None,
        None,
        None,
        None,
        true,
        Some("appstore"), // macos appstore, no creds
        false,
        None,
        false,
        None,
    );
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("App Store Connect API credentials"), "{msg}");
    assert!(msg.contains("perry setup macos"), "{msg}");
}

#[test]
fn test_validate_macos_testflight_requires_creds() {
    let result = validate_credentials_for_distribute(
        false,
        None,
        None,
        false,
        None,
        None,
        None,
        None,
        true,
        Some("testflight"), // macos testflight, no creds
        false,
        None,
        false,
        None,
    );
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("App Store Connect API credentials"), "{msg}");
}

#[test]
fn test_validate_macos_notarize_requires_creds() {
    let result = validate_credentials_for_distribute(
        false,
        None,
        None,
        false,
        None,
        None,
        None,
        None,
        true,
        Some("notarize"), // macos notarize, no creds
        false,
        None,
        false,
        None,
    );
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("notarization"), "{msg}");
}

#[test]
fn test_validate_passes_when_all_present() {
    let result = validate_credentials_for_distribute(
        false,
        None,
        None,
        true,
        Some("appstore"),
        Some("kid"),
        Some("iid"),
        Some("p8"),
        false,
        None,
        false,
        None,
        false,
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_validate_tvos_appstore_requires_creds() {
    let result = validate_credentials_for_distribute(
        false,
        None,
        None,
        false,
        None,
        None,
        None,
        None,
        false,
        None,
        true,
        Some("appstore"), // tvos appstore, no creds
        false,
        None,
    );
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("App Store Connect API credentials"), "{msg}");
    assert!(msg.contains("perry setup tvos"), "{msg}");
}

#[test]
fn test_validate_watchos_appstore_requires_creds() {
    let result = validate_credentials_for_distribute(
        false,
        None,
        None,
        false,
        None,
        None,
        None,
        None,
        false,
        None,
        false,
        None,
        true,
        Some("appstore"), // watchos appstore, no creds
    );
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("App Store Connect API credentials"), "{msg}");
    assert!(msg.contains("perry setup watchos"), "{msg}");
}

#[test]
fn test_validate_watchos_testflight_missing_issuer() {
    let result = validate_credentials_for_distribute(
        false,
        None,
        None,
        false,
        None,
        Some("kid"),
        None,
        Some("p8"),
        false,
        None,
        false,
        None,
        true,
        Some("testflight"), // watchos testflight, issuer missing
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Issuer ID"));
}

#[test]
fn test_validate_watchos_no_distribute_passes() {
    let result = validate_credentials_for_distribute(
        false, None, None, false, None, None, None, None, false, None, false, None, true,
        None, // watchos but no distribute set
    );
    assert!(result.is_ok());
}

// --- GHSA-x55v-q459-68ch: server-controlled artifact path handling ---

#[test]
fn test_sanitize_artifact_name_accepts_plain_names() {
    assert_eq!(sanitize_artifact_name("app.zip").unwrap(), "app.zip");
    assert_eq!(
        sanitize_artifact_name("MyGame-1.2.3.dmg").unwrap(),
        "MyGame-1.2.3.dmg"
    );
    // Surrounding whitespace is trimmed, not treated as part of the name.
    assert_eq!(sanitize_artifact_name("  app.ipa  ").unwrap(), "app.ipa");
}

#[test]
fn test_sanitize_artifact_name_rejects_traversal() {
    for evil in [
        "../../.ssh/authorized_keys",
        "../x",
        "a/b",
        "a/../b",
        "/etc/cron.d/x",
        "/etc/passwd",
        "..",
        ".",
        "",
        "   ",
        "foo/bar.zip",
        "..\\windows\\system32",
        "C:\\Users\\victim\\evil.exe",
    ] {
        assert!(
            sanitize_artifact_name(evil).is_err(),
            "expected {evil:?} to be rejected as unsafe"
        );
    }
}

#[test]
fn test_server_is_local() {
    assert!(server_is_local("http://localhost:3000"));
    assert!(server_is_local("https://LOCALHOST"));
    assert!(server_is_local("http://127.0.0.1:8080"));
    assert!(server_is_local("http://[::1]:9000"));

    assert!(!server_is_local("https://hub.perryts.com"));
    assert!(!server_is_local("https://attacker.example.com"));
    assert!(!server_is_local("http://10.0.0.5:3000"));
    assert!(!server_is_local("not a url"));
}

#[test]
fn test_resolve_build_march_precedence_and_linux_default() {
    // #6125 — CLI --march > [build] march > [build] native_tuning shorthand
    // > linux portable default > nothing.
    fn bc(march: Option<&str>, native_tuning: Option<bool>) -> config_types::BuildConfig {
        config_types::BuildConfig {
            out_dir: None,
            march: march.map(str::to_string),
            native_tuning,
        }
    }

    // CLI flag wins over everything.
    let cfg = bc(Some("x86-64-v3"), Some(false));
    assert_eq!(
        resolve_build_march(Some("znver2"), Some(&cfg), true).as_deref(),
        Some("znver2")
    );
    // [build] march wins over native_tuning.
    assert_eq!(
        resolve_build_march(None, Some(&cfg), true).as_deref(),
        Some("x86-64-v3")
    );
    // native_tuning shorthand: false → generic, true → native.
    assert_eq!(
        resolve_build_march(None, Some(&bc(None, Some(false))), true).as_deref(),
        Some("generic")
    );
    assert_eq!(
        resolve_build_march(None, Some(&bc(None, Some(true))), true).as_deref(),
        Some("native")
    );
    // Unset on linux → portable x86-64-v2 default (whitespace-only march
    // counts as unset).
    assert_eq!(
        resolve_build_march(None, None, true).as_deref(),
        Some("x86-64-v2")
    );
    assert_eq!(
        resolve_build_march(Some("  "), Some(&bc(Some(" "), None)), true).as_deref(),
        Some("x86-64-v2")
    );
    // Unset on a non-linux target → nothing is sent.
    assert_eq!(resolve_build_march(None, None, false), None);
}
