//! Distinct symbol prefixes for modules whose names sanitize alike.
//!
//! Codegen prefixes every module symbol with the module name, sanitized to
//! an identifier (`sanitize(hir.name)`): every character other than ASCII
//! letters, digits and `_` becomes `_`. That map is not injective:
//! `providers/google-vertex-chat.ts` and `providers/google-vertex/chat.ts`
//! both become `providers_google_vertex_chat_ts`, and the two modules' symbols
//! merge at link time (one module's namespace getter referenced the other's
//! re-export getter, and the link failed).
//!
//! Once the whole program is collected, every module in a colliding group
//! gets its name suffixed with `#<hash of the name>`, so its sanitized name,
//! and with it every prefix codegen derives, is distinct. The driver derives
//! prefixes from module paths, so the new prefixes are recorded by path in
//! `module_prefix_overrides`, which `compute_module_prefix` consults. Modules
//! whose names do not collide keep their names and prefixes.

use std::collections::{BTreeMap, HashSet};

use anyhow::{bail, Result};
use sha2::{Digest, Sha256};

use super::CompilationContext;

/// Codegen's module-prefix sanitizer (`perry_codegen::codegen::helpers::sanitize`).
pub(super) fn sanitize(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

pub(super) fn disambiguate(ctx: &mut CompilationContext) -> Result<()> {
    let mut groups: BTreeMap<String, Vec<std::path::PathBuf>> = BTreeMap::new();
    for (path, module) in &ctx.native_modules {
        groups
            .entry(sanitize(&module.name))
            .or_default()
            .push(path.clone());
    }
    for paths in groups.into_values().filter(|paths| paths.len() > 1) {
        for path in paths {
            let module = ctx
                .native_modules
                .get_mut(&path)
                .expect("grouped module is collected");
            let digest = Sha256::digest(module.name.as_bytes());
            let tag: String = digest[..4].iter().map(|b| format!("{b:02x}")).collect();
            module.name = format!("{}#{tag}", module.name);
            ctx.module_prefix_overrides
                .insert(path.to_string_lossy().into_owned(), sanitize(&module.name));
        }
    }
    let mut seen = HashSet::new();
    for module in ctx.native_modules.values() {
        if !seen.insert(sanitize(&module.name)) {
            bail!(
                "two modules still share the symbol prefix `{}` after disambiguation (module {})",
                sanitize(&module.name),
                module.name
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    #[test]
    fn sanitize_matches_codegen_and_is_lossy() {
        assert_eq!(
            sanitize("a/google-vertex-chat.ts"),
            "a_google_vertex_chat_ts"
        );
        assert_eq!(
            sanitize("a/google-vertex/chat.ts"),
            "a_google_vertex_chat_ts"
        );
        assert_eq!(sanitize("05_fib.ts"), "_05_fib_ts");
    }
}
