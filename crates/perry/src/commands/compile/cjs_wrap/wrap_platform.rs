//! Target-dependent CommonJS platform and native-watcher resolution.

use super::extract_require_specifiers;
fn target_node_platform(target: Option<&str>) -> Option<&'static str> {
    if crate::commands::compile::is_windows_target(target) {
        return Some("win32");
    }
    match target {
        Some("linux") | Some("linux-x86_64") | Some("linux-arm64") | Some("linux-aarch64")
        // musl shares node's `process.platform === "linux"` (#4826).
        | Some("linux-musl") | Some("linux-x86_64-musl") | Some("linux-aarch64-musl") => {
            Some("linux")
        }
        Some("macos")
        | Some("ios")
        | Some("ios-simulator")
        | Some("ios-widget")
        | Some("ios-widget-simulator")
        | Some("visionos")
        | Some("visionos-simulator")
        | Some("watchos")
        | Some("watchos-simulator")
        | Some("watchos-widget")
        | Some("watchos-widget-simulator")
        | Some("tvos")
        | Some("tvos-simulator") => Some("darwin"),
        Some(_) => None,
        None => {
            #[cfg(target_os = "windows")]
            {
                Some("win32")
            }
            #[cfg(target_os = "linux")]
            {
                Some("linux")
            }
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            {
                Some("darwin")
            }
            #[cfg(not(any(
                target_os = "windows",
                target_os = "linux",
                target_os = "macos",
                target_os = "ios"
            )))]
            {
                None
            }
        }
    }
}

fn target_node_arch(target: Option<&str>) -> Option<&'static str> {
    match target {
        Some(value) if value.contains("x86_64") || value.contains("x64") => Some("x64"),
        Some(value) if value.contains("aarch64") || value.contains("arm64") => Some("arm64"),
        Some("windows") | Some("linux") | Some("linux-musl") | Some("macos") => host_node_arch(),
        Some(_) => None,
        None => host_node_arch(),
    }
}

fn host_node_arch() -> Option<&'static str> {
    #[cfg(target_arch = "x86_64")]
    {
        return Some("x64");
    }
    #[cfg(target_arch = "aarch64")]
    {
        return Some("arm64");
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        None
    }
}

/// The registered `@parcel/watcher` facade entry points. They stay native even
/// under an explicit `compilePackages` opt-in, so both the generated import and
/// the generated `require.resolve` identity keep the package specifier instead
/// of resolving it to `watcher.node`.
pub(super) fn is_watcher_facade(spec: &str) -> bool {
    perry_hir::is_native_module(spec)
        && (spec == "@parcel/watcher" || spec.starts_with("@parcel/watcher-"))
}

/// Fold OpenCode's target-dependent @parcel/watcher sidecar require before
/// the ordinary literal-require extractor runs. Native build targets make
/// process.platform/process.arch/libc constants, so this is the same branch
/// selection Node's package loader would perform at startup.
pub(super) fn fold_parcel_watcher_template_require(
    source: &str,
    target: Option<&str>,
) -> Option<String> {
    let platform = target_node_platform(target)?;
    let arch = target_node_arch(target)?;
    let suffix = if platform == "linux" {
        if target.is_some_and(|value| value.contains("musl")) {
            "-musl"
        } else {
            "-glibc"
        }
    } else {
        ""
    };
    let specifier = format!("@parcel/watcher-{platform}-{arch}{suffix}");
    let template = perry_perex::tooling::Regex::new(
        r#"`@parcel/watcher-\$\{process\.platform\}-\$\{process\.arch\}\$\{process\.platform\s*===\s*[\"']linux[\"']\s*\?\s*`-\$\{libc\s*\|\|\s*[\"']glibc[\"']\}`\s*:\s*[\"'][\"']\}`"#,
    )
    .expect("parcel watcher template regex");
    if !template.is_match(source) {
        return None;
    }
    Some(
        template
            .replace_all(source, format!("\"{specifier}\"").as_str())
            .into_owned(),
    )
}

pub(super) fn inactive_platform_guarded_requires(
    source: &str,
    target: Option<&str>,
) -> std::collections::HashSet<String> {
    let Some(platform) = target_node_platform(target) else {
        return std::collections::HashSet::new();
    };
    let re = perry_perex::tooling::Regex::new(
        r#"(?s)if\s*\(\s*process\.platform\s*(===|!==)\s*['"]([^'"]+)['"]\s*\)\s*\{(?P<then>.*?)\}\s*else\s*\{(?P<else>.*?)\}"#,
    )
    .unwrap();
    let mut inactive = std::collections::HashSet::new();
    for cap in re.captures_iter(source) {
        let Some(op) = cap.get(1).map(|m| m.as_str()) else {
            continue;
        };
        let Some(expected) = cap.get(2).map(|m| m.as_str()) else {
            continue;
        };
        let condition_true = match op {
            "===" => platform == expected,
            "!==" => platform != expected,
            _ => continue,
        };
        let dead_body = if condition_true {
            cap.name("else")
        } else {
            cap.name("then")
        };
        if let Some(body) = dead_body {
            inactive.extend(extract_require_specifiers(body.as_str()));
        }
    }
    inactive
}
