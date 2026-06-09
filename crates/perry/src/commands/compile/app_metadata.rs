//! App metadata + target/triple helpers extracted from `compile.rs`.
//!
//! Pure helpers that map between perry.toml + package.json + CLI flags and
//! the `perry_codegen::AppMetadata` struct that codegen consumes. Also home
//! to `rust_target_triple` because every sibling under `compile/` already
//! reaches for it via `super::rust_target_triple(...)` — keeping it adjacent
//! to the bundle-id resolution code is the natural fit.

use std::fs;
use std::path::Path;

pub(super) fn target_bundle_section(target: Option<&str>) -> Option<&'static str> {
    match target {
        Some("ios") | Some("ios-simulator") => Some("ios"),
        Some("visionos") | Some("visionos-simulator") => Some("visionos"),
        Some("watchos") | Some("watchos-simulator") => Some("watchos"),
        Some("tvos") | Some("tvos-simulator") => Some("tvos"),
        Some("android") => Some("android"),
        Some("macos") => Some("macos"),
        // WinUI shares the [windows] perry.toml section (#4680).
        Some("windows") | Some("windows-winui") => Some("windows"),
        // musl variants share the [linux] perry.toml section (#4826).
        Some("linux")
        | Some("linux-musl")
        | Some("linux-x86_64-musl")
        | Some("linux-aarch64-musl") => Some("linux"),
        None if cfg!(target_os = "macos") => Some("macos"),
        None if cfg!(target_os = "windows") => Some("windows"),
        None if cfg!(target_os = "linux") => Some("linux"),
        _ => None,
    }
}

pub(super) fn toml_string(table: &toml::Table, section: &str, key: &str) -> Option<String> {
    table
        .get(section)
        .and_then(|v| v.as_table())
        .and_then(|s| s.get(key))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

pub(super) fn toml_build_number(table: &toml::Table) -> Option<i64> {
    let value = table
        .get("project")
        .and_then(|v| v.as_table())
        .and_then(|project| project.get("build_number"))?;
    value
        .as_integer()
        .or_else(|| value.as_str().and_then(|s| s.parse::<i64>().ok()))
}

fn package_bundle_id_from_input(input: &Path) -> Option<String> {
    let mut dir = input.canonicalize().ok()?;
    if dir.is_file() {
        dir = dir.parent()?.to_path_buf();
    }
    loop {
        let pkg = dir.join("package.json");
        if pkg.exists() {
            let data = fs::read_to_string(&pkg).ok()?;
            let json: serde_json::Value = serde_json::from_str(&data).ok()?;
            if let Some(bundle_id) = json.get("bundleId").and_then(|v| v.as_str()) {
                // #999: explicit `bundleId` was previously passed straight to
                // codesign argv; reject obviously-hostile values at read time
                // with a clear file-path diagnostic instead of silently letting
                // them through.
                let label = format!("package.json `bundleId` at {}", pkg.display());
                return Some(crate::commands::sanitize::validate_bundle_id_or_exit(
                    bundle_id, &label,
                ));
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

pub(super) fn read_app_metadata(
    perry_toml: Option<&toml::Table>,
    input: &Path,
    target: Option<&str>,
    cli_bundle_id: Option<&str>,
) -> perry_codegen::AppMetadata {
    let mut metadata = perry_codegen::AppMetadata::default();

    if let Some(doc) = perry_toml {
        if let Some(version) = toml_string(doc, "project", "version") {
            metadata.version = version;
        }
        if let Some(build_number) = toml_build_number(doc) {
            metadata.build_number = build_number;
        }
        // #1178 — App Group suite name. Resolution order: target-specific
        // section (`[ios]` / `[macos]`) → generic `[app] app_group` →
        // top-level `app_group`. The section preference for Apple targets
        // mirrors how `bundle_id` is resolved above; non-Apple targets
        // still see the value so `[android] app_group` can flow through
        // once the SharedPreferences backend lands.
        let target_section = target_bundle_section(target);
        metadata.app_group = target_section
            .and_then(|section| toml_string(doc, section, "app_group"))
            .or_else(|| toml_string(doc, "app", "app_group"))
            .or_else(|| {
                doc.get("app_group")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            });
    }

    metadata.bundle_id = cli_bundle_id
        .map(|raw| {
            // #999: CLI `--app-bundle-id` flag goes straight to codesign argv;
            // validate before accepting it.
            crate::commands::sanitize::validate_bundle_id_or_exit(raw, "CLI --app-bundle-id")
        })
        .or_else(|| {
            let doc = perry_toml?;
            let raw = target_bundle_section(target)
                .and_then(|section| toml_string(doc, section, "bundle_id"))
                .or_else(|| toml_string(doc, "app", "bundle_id"))
                .or_else(|| toml_string(doc, "project", "bundle_id"))
                .or_else(|| {
                    doc.get("bundle_id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                })?;
            // #999: perry.toml is host-trusted in normal use, but the threat
            // model in sanitize.rs explicitly lists it as attacker-influenceable
            // (a hostile dep dropping perry.toml in a project root). Validate
            // before letting it reach codesign.
            Some(crate::commands::sanitize::validate_bundle_id_or_exit(
                &raw,
                "perry.toml `bundle_id`",
            ))
        })
        .or_else(|| package_bundle_id_from_input(input))
        .unwrap_or_else(|| {
            // Issue #500: bundle_id flows into codesign / productbuild
            // argv. The input file stem is attacker-influenceable
            // (tooling-chosen paths), so route it through the shared
            // sanitizer before splicing into the reverse-DNS string.
            // The helper is also used by every other Apple-platform
            // fallback site (#998) so generated bundle IDs agree.
            let raw = input.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
            crate::commands::sanitize::default_perry_bundle_id(raw)
        });

    metadata
}

/// Get the Rust target triple for a given perry target string
pub(super) fn rust_target_triple(target: Option<&str>) -> Option<&'static str> {
    match target {
        Some("ios-simulator") | Some("ios-widget-simulator") => Some("aarch64-apple-ios-sim"),
        Some("ios") | Some("ios-widget") => Some("aarch64-apple-ios"),
        Some("visionos-simulator") => Some("aarch64-apple-visionos-sim"),
        Some("visionos") => Some("aarch64-apple-visionos"),
        Some("watchos-simulator") => Some("aarch64-apple-watchos-sim"),
        Some("watchos") => Some("arm64_32-apple-watchos"),
        Some("tvos-simulator") => Some("aarch64-apple-tvos-sim"),
        Some("tvos") => Some("aarch64-apple-tvos"),
        Some("harmonyos") => Some("aarch64-unknown-linux-ohos"),
        Some("harmonyos-simulator") => Some("x86_64-unknown-linux-ohos"),
        Some("android") => Some("aarch64-linux-android"),
        Some("linux") | Some("linux-x86_64") => Some("x86_64-unknown-linux-gnu"),
        Some("linux-arm64") | Some("linux-aarch64") => Some("aarch64-unknown-linux-gnu"),
        // Fully-static musl targets (#4826). The perry-runtime / perry-stdlib
        // .a files for these triples are built by release-packages.yml.
        Some("linux-musl") | Some("linux-x86_64-musl") => Some("x86_64-unknown-linux-musl"),
        Some("linux-aarch64-musl") => Some("aarch64-unknown-linux-musl"),
        Some("windows") | Some("windows-winui") => Some("x86_64-pc-windows-msvc"),
        Some("macos") => Some("aarch64-apple-darwin"),
        _ => None,
    }
}

#[cfg(test)]
mod app_metadata_tests {
    use super::read_app_metadata;

    fn parse(src: &str) -> toml::Table {
        src.parse::<toml::Table>().unwrap()
    }

    #[test]
    fn reads_project_metadata_and_target_bundle_id() {
        let dir = tempfile::tempdir().unwrap();
        let doc = parse(
            r#"
[project]
version = "2.4.6"
build_number = 42
bundle_id = "com.example.project"

[ios]
bundle_id = "com.example.ios"
"#,
        );
        let input = dir.path().join("src").join("main.ts");
        std::fs::create_dir_all(input.parent().unwrap()).unwrap();
        std::fs::write(&input, "console.log('x')").unwrap();

        let metadata = read_app_metadata(Some(&doc), &input, Some("ios-simulator"), None);

        assert_eq!(metadata.version, "2.4.6");
        assert_eq!(metadata.build_number, 42);
        assert_eq!(metadata.bundle_id, "com.example.ios");
    }

    #[test]
    fn cli_bundle_id_overrides_toml_bundle_id() {
        let dir = tempfile::tempdir().unwrap();
        let doc = parse(
            r#"
[project]
version = "1.0.0"
build_number = "7"
bundle_id = "com.example.project"
"#,
        );
        let input = dir.path().join("main.ts");
        std::fs::write(&input, "console.log('x')").unwrap();

        let metadata = read_app_metadata(Some(&doc), &input, Some("ios"), Some("com.example.cli"));

        assert_eq!(metadata.version, "1.0.0");
        assert_eq!(metadata.build_number, 7);
        assert_eq!(metadata.bundle_id, "com.example.cli");
    }

    #[test]
    fn reads_ios_app_group_from_target_section() {
        // #1178 — `[ios] app_group` is the canonical place to declare the
        // App Group suite name; it should win over the generic `[app]`
        // fallback for an iOS target.
        let dir = tempfile::tempdir().unwrap();
        let doc = parse(
            r#"
[ios]
app_group = "group.com.example.shared"

[app]
app_group = "group.com.example.fallback"
"#,
        );
        let input = dir.path().join("main.ts");
        std::fs::write(&input, "console.log('x')").unwrap();

        let ios = read_app_metadata(Some(&doc), &input, Some("ios"), None);
        assert_eq!(ios.app_group.as_deref(), Some("group.com.example.shared"));

        // No-target build (host = macOS in CI) should still pick the
        // `[macos]` or fall back to `[app]` when the target section is
        // absent — here only `[app]` provides a value for the implicit
        // host target.
        let host = read_app_metadata(Some(&doc), &input, None, None);
        // macOS host: `[macos]` is absent, so `[app]` wins.
        if cfg!(target_os = "macos") {
            assert_eq!(
                host.app_group.as_deref(),
                Some("group.com.example.fallback")
            );
        }
    }

    #[test]
    fn missing_app_group_is_none() {
        // No perry.toml at all → app_group stays None, which the codegen
        // prelude reads as "skip the perry_app_group_init() call" and the
        // runtime treats as "stub-warn on first appGroupSet".
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("main.ts");
        std::fs::write(&input, "console.log('x')").unwrap();
        let metadata = read_app_metadata(None, &input, Some("ios"), None);
        assert!(metadata.app_group.is_none());
    }

    #[test]
    fn package_json_bundle_id_falls_back_when_perry_toml_silent_on_bundle() {
        // No perry.toml at all — bundle_id should be read from package.json's
        // `bundleId` field walking up parents from the input file. Version and
        // build_number stay on defaults (no perry.toml).
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"bundleId": "com.example.pkg"}"#,
        )
        .unwrap();
        let input = dir.path().join("src").join("main.ts");
        std::fs::create_dir_all(input.parent().unwrap()).unwrap();
        std::fs::write(&input, "console.log('x')").unwrap();

        let metadata = read_app_metadata(None, &input, None, None);

        assert_eq!(metadata.version, "1.0.0");
        assert_eq!(metadata.build_number, 1);
        assert_eq!(metadata.bundle_id, "com.example.pkg");
    }

    #[test]
    fn musl_targets_resolve_consistently() {
        // #4826: the musl target names must map to the musl rustc triple,
        // share the [linux] perry.toml section, and agree with the codegen
        // LLVM-triple resolver — otherwise library_search / codegen / link
        // disagree about which target/<triple>/release dir to use.
        for t in ["linux-musl", "linux-x86_64-musl"] {
            assert_eq!(
                super::rust_target_triple(Some(t)),
                Some("x86_64-unknown-linux-musl"),
                "rust_target_triple({t})"
            );
            assert_eq!(
                perry_codegen::resolve_target_triple(t).as_deref(),
                Some("x86_64-unknown-linux-musl"),
                "resolve_target_triple({t})"
            );
            assert_eq!(super::target_bundle_section(Some(t)), Some("linux"));
        }
        assert_eq!(
            super::rust_target_triple(Some("linux-aarch64-musl")),
            Some("aarch64-unknown-linux-musl")
        );
        assert_eq!(
            perry_codegen::resolve_target_triple("linux-aarch64-musl").as_deref(),
            Some("aarch64-unknown-linux-musl")
        );
        assert_eq!(
            super::target_bundle_section(Some("linux-aarch64-musl")),
            Some("linux")
        );
    }

    #[test]
    fn libc_flag_upgrades_linux_targets_to_musl() {
        use crate::commands::compile::apply_libc_to_target;
        let m = |t: Option<&str>, libc: Option<&str>| {
            apply_libc_to_target(t.map(str::to_string), libc).unwrap()
        };
        // No flag / glibc → unchanged.
        assert_eq!(m(Some("linux"), None), Some("linux".to_string()));
        assert_eq!(m(Some("linux"), Some("glibc")), Some("linux".to_string()));
        // musl upgrades the Linux variants.
        assert_eq!(m(None, Some("musl")), Some("linux-musl".to_string()));
        assert_eq!(
            m(Some("linux"), Some("musl")),
            Some("linux-musl".to_string())
        );
        assert_eq!(
            m(Some("linux-aarch64"), Some("musl")),
            Some("linux-aarch64-musl".to_string())
        );
        // Idempotent.
        assert_eq!(
            m(Some("linux-musl"), Some("musl")),
            Some("linux-musl".to_string())
        );
        // Non-Linux target + musl, and unknown libc, are hard errors.
        assert!(apply_libc_to_target(Some("windows".into()), Some("musl")).is_err());
        assert!(apply_libc_to_target(Some("linux".into()), Some("uclibc")).is_err());
    }
}
