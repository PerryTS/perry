//! Solid universal renderer selection, shared by module collection and resolution.

use std::path::Path;

use anyhow::{bail, Result};

#[derive(Clone, Debug, Default)]
pub(crate) enum JsxMode {
    #[default]
    Auto,
    Default,
    Solid(String),
}

impl JsxMode {
    pub(super) fn parse(value: &serde_json::Value) -> Result<Self> {
        match value.as_str() {
            Some("solid") => return Ok(Self::Solid("perry-solid".into())),
            Some("default") => return Ok(Self::Default),
            _ => {}
        }
        if let Some(runtime) = value.get("runtime").and_then(|v| v.as_str()) {
            if !runtime.trim().is_empty() && runtime == runtime.trim() {
                return Ok(Self::Solid(runtime.to_owned()));
            }
        }
        bail!("perry.jsx must be \"solid\", \"default\", or {{ \"runtime\": \"module-name\" }}")
    }

    pub(super) fn runtime_for(&self, source: &Path) -> Result<Option<String>> {
        match self {
            Self::Default => return Ok(None),
            Self::Solid(runtime) => return Ok(Some(runtime.clone())),
            Self::Auto => {}
        }
        // A monorepo can mix JSX dialects. Honor the nearest package's explicit
        // choice; dependencies do not inherit a host package across node_modules.
        if let Some(dir) = source.parent() {
            for dir in dir.ancestors() {
                if dir.file_name().is_some_and(|name| name == "node_modules") {
                    break;
                }
                let package = dir.join("package.json");
                if package.is_file() {
                    let json = std::fs::read_to_string(package)
                        .ok()
                        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
                    if let Some(value) = json
                        .as_ref()
                        .and_then(|json| json.get("perry"))
                        .and_then(|p| p.get("jsx"))
                    {
                        return Self::parse(value)?.runtime_for(source);
                    }
                    break;
                }
            }
        }
        let (source, _) = super::resolve::tsconfig_paths::jsx_config(source);
        // jsxImportSource also names React/Preact and Solid's DOM renderer.
        // Only known universal hosts opt in automatically; other universal
        // renderers can be selected explicitly with perry.jsx.runtime.
        Ok(source.filter(|source| matches!(source.as_str(), "@opentui/solid" | "perry-solid")))
    }
}

#[cfg(test)]
mod tests;
