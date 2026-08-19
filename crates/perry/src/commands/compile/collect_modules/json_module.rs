//! JSON-module source synthesis.
//!
//! Keep JSON as a serialized string until runtime. Lowering a large JSON object
//! literal emits an allocation and property-store sequence for every member;
//! data sets such as `mime-db` consequently produced megabytes of machine code.

use anyhow::{anyhow, Result};
use std::path::Path;

pub(super) fn synthesize_json_module(raw: &str, canonical: &Path) -> Result<String> {
    serde_json::from_str::<serde_json::Value>(raw).map_err(|error| {
        anyhow!(
            "Failed to parse JSON module {}: {}",
            canonical.display(),
            error
        )
    })?;

    // serde_json's string serializer produces a valid JavaScript literal and
    // escapes control characters, quotes, backslashes, and line separators.
    let serialized = serde_json::to_string(raw.trim()).map_err(|error| {
        anyhow!(
            "Failed to encode JSON module {}: {}",
            canonical.display(),
            error
        )
    })?;
    let path = canonical.to_string_lossy();

    Ok(format!(
        "function __perry_json_factory() {{ return JSON.parse({serialized}); }}\n\
         const __perry_json_default = __perry_json_factory();\n\
         const __perry_json_module = {{ __perry_cjs_record: true, __perry_cjs_factory: __perry_json_factory, exports: __perry_json_default, loaded: false }};\n\
         __perry_register_path_module({path:?}, __perry_json_module);\n\
         export default __perry_json_default;\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::synthesize_json_module;
    use std::path::Path;

    #[test]
    fn keeps_json_serialized_instead_of_lowering_an_object_literal() {
        let source = synthesize_json_module(
            r#"{"items":[1,true,null],"nested":{"message":"hello"}}"#,
            Path::new("data.json"),
        )
        .expect("synthesize JSON module");

        assert!(source.contains("return JSON.parse("));
        assert!(!source.contains("return {\"items\""));
        assert!(source.contains("export default __perry_json_default"));
    }

    #[test]
    fn rejects_invalid_json_before_typescript_parsing() {
        let error = synthesize_json_module("{broken", Path::new("broken.json"))
            .expect_err("invalid JSON should fail");
        assert!(error.to_string().contains("Failed to parse JSON module"));
        assert!(error.to_string().contains("broken.json"));
    }
}
