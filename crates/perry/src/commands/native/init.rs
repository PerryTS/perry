//! `perry native init <name>` — scaffold a new native-bindings package.

use anyhow::{anyhow, Context, Result};
use clap::Args;
use std::fs;
use std::path::{Path, PathBuf};

use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Package name (also the directory created and the npm
    /// package name unless `--npm-name` is set). For scoped names
    /// like `@perryts/iroh`, pass the scope and the directory will
    /// be named after the bare segment.
    pub name: String,

    /// Override the npm package name (defaults to `name`).
    #[arg(long)]
    pub npm_name: Option<String>,

    /// One-line description for package.json + README + LICENSE.
    #[arg(long)]
    pub description: Option<String>,

    /// GitHub `<owner>` for the README + Cargo.toml repository links.
    /// Defaults to `your-github-handle` (placeholder).
    #[arg(long)]
    pub github_owner: Option<String>,

    /// Upstream Rust crate to wrap, e.g. `tokio-tungstenite = "0.24"`
    /// or `mongodb = { version = "3.5", features = [...] }`. Pasted
    /// verbatim into the new Cargo.toml's `[dependencies]` block.
    #[arg(long)]
    pub upstream_dep: Option<String>,

    /// Copyright holder for the LICENSE file. Defaults to the
    /// `github_owner` value.
    #[arg(long)]
    pub copyright_holder: Option<String>,

    /// Overwrite an existing directory of the same name (default:
    /// abort to avoid clobbering work).
    #[arg(long)]
    pub force: bool,
}

// ── embedded templates ────────────────────────────────────────────

const TPL_PKG: &str = include_str!("../../../templates/native-package/package.json.tmpl");
const TPL_CARGO: &str = include_str!("../../../templates/native-package/Cargo.toml.tmpl");
const TPL_LIB_RS: &str = include_str!("../../../templates/native-package/src/lib.rs.tmpl");
const TPL_INDEX_TS: &str = include_str!("../../../templates/native-package/src/index.ts.tmpl");
const TPL_README: &str = include_str!("../../../templates/native-package/README.md.tmpl");
const TPL_LICENSE: &str = include_str!("../../../templates/native-package/LICENSE");
const TPL_GITIGNORE: &str = include_str!("../../../templates/native-package/gitignore");
const TPL_RELEASE_YML: &str =
    include_str!("../../../templates/native-package/.github/workflows/release.yml");

pub fn run(args: InitArgs, format: OutputFormat, _use_color: bool) -> Result<()> {
    // Bare segment after any `@scope/` prefix becomes the
    // directory name + the Cargo crate's identifier root.
    let name = args.name.trim();
    if name.is_empty() {
        return Err(anyhow!("`perry native init` requires a package name"));
    }
    let dir_name = match name.split_once('/') {
        Some((scope, bare)) if scope.starts_with('@') => bare,
        _ => name.strip_prefix('@').unwrap_or(name),
    }
    .to_string();
    let crate_name = sanitize_crate_name(&dir_name);
    let rust_crate_name = format!("perry-ext-{}", crate_name);
    let target = PathBuf::from(&dir_name);

    if target.exists() && !args.force {
        return Err(anyhow!(
            "directory `{}` already exists; pass --force to overwrite",
            target.display()
        ));
    }

    let npm_name = args.npm_name.unwrap_or_else(|| name.to_string());
    let github_owner = args
        .github_owner
        .unwrap_or_else(|| "your-github-handle".to_string());
    let copyright = args
        .copyright_holder
        .unwrap_or_else(|| github_owner.clone());
    let description = args
        .description
        .unwrap_or_else(|| format!("Native bindings for {} for the Perry compiler.", crate_name));
    let year = current_year();

    // The Cargo.toml template has a `{{upstream_dep_line}}` slot — let
    // `--upstream-dep "foo = \"1.0\""` flow straight in, or leave a
    // commented placeholder for the user to fill.
    let upstream_dep_line = match args.upstream_dep.as_deref() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => format!(
            "# TODO: add the upstream Rust crate to wrap, e.g. `{} = \"1.0\"`",
            crate_name
        ),
    };

    let subs = Subs {
        npm_name: &npm_name,
        crate_name: &crate_name,
        rust_crate_name: &rust_crate_name,
        github_owner: &github_owner,
        repo_name: &dir_name,
        copyright_holder: &copyright,
        description: &description,
        year: &year,
        upstream_dep_line: &upstream_dep_line,
    };

    write_template(&target.join("package.json"), TPL_PKG, &subs)?;
    write_template(&target.join("Cargo.toml"), TPL_CARGO, &subs)?;
    write_template(&target.join("src/lib.rs"), TPL_LIB_RS, &subs)?;
    write_template(&target.join("src/index.ts"), TPL_INDEX_TS, &subs)?;
    write_template(&target.join("README.md"), TPL_README, &subs)?;
    write_template(&target.join("LICENSE"), TPL_LICENSE, &subs)?;
    write_raw(&target.join(".gitignore"), TPL_GITIGNORE)?;
    write_raw(
        &target.join(".github/workflows/release.yml"),
        TPL_RELEASE_YML,
    )?;

    if matches!(format, OutputFormat::Text) {
        println!("Created {} at {}", npm_name, target.display());
        println!();
        println!("Next steps:");
        println!("  cd {}", dir_name);
        println!("  # Edit Cargo.toml to add your upstream Rust crate dep.");
        println!("  # Edit src/lib.rs to add `js_*` functions for each binding.");
        println!("  # Edit src/index.ts to declare the TypeScript surface.");
        println!("  # Edit package.json's perry.nativeLibrary.functions[] to match.");
        println!("  cargo build --release");
        println!("  perry native validate");
        println!();
        println!("Then publish:");
        println!("  npm publish");
        println!("  git tag v0.1.0 && git push --tags  # triggers release.yml prebuilds");
    }

    Ok(())
}

struct Subs<'a> {
    npm_name: &'a str,
    crate_name: &'a str,
    rust_crate_name: &'a str,
    github_owner: &'a str,
    repo_name: &'a str,
    copyright_holder: &'a str,
    description: &'a str,
    year: &'a str,
    upstream_dep_line: &'a str,
}

fn render(template: &str, subs: &Subs) -> String {
    template
        .replace("{{npm_name}}", subs.npm_name)
        .replace("{{crate_name}}", subs.crate_name)
        .replace("{{rust_crate_name}}", subs.rust_crate_name)
        .replace("{{github_owner}}", subs.github_owner)
        .replace("{{repo_name}}", subs.repo_name)
        .replace("{{copyright_holder}}", subs.copyright_holder)
        .replace("{{description}}", subs.description)
        .replace("{{year}}", subs.year)
        .replace("{{upstream_dep_line}}", subs.upstream_dep_line)
}

fn write_template(path: &Path, template: &str, subs: &Subs) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(path, render(template, subs))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn write_raw(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn sanitize_crate_name(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn current_year() -> String {
    use chrono::Datelike;
    chrono::Utc::now().year().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_substitutes_all_slots() {
        let subs = Subs {
            npm_name: "@scope/foo",
            crate_name: "foo",
            rust_crate_name: "perry-ext-foo",
            github_owner: "alice",
            repo_name: "foo",
            copyright_holder: "Alice",
            description: "demo bindings",
            year: "2026",
            upstream_dep_line: "foo = \"1.0\"",
        };
        let out = render(
            "name={{npm_name}} crate={{crate_name}} desc={{description}} year={{year}}",
            &subs,
        );
        assert_eq!(
            out,
            "name=@scope/foo crate=foo desc=demo bindings year=2026"
        );
    }

    #[test]
    fn sanitize_crate_name_dashes_to_underscores() {
        assert_eq!(sanitize_crate_name("foo-bar"), "foo_bar");
        assert_eq!(sanitize_crate_name("baz.qux"), "baz_qux");
        assert_eq!(sanitize_crate_name("clean"), "clean");
    }

    #[test]
    fn current_year_is_4_digits() {
        let y = current_year();
        assert_eq!(y.len(), 4);
        assert!(y.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn package_template_points_resolvers_at_ts_surface() {
        assert!(TPL_PKG.contains(r#""main": "src/index.ts""#));
        assert!(TPL_PKG.contains(r#""types": "src/index.ts""#));
    }

    #[test]
    fn index_template_forwards_to_manifest_symbol() {
        assert!(TPL_INDEX_TS.contains("declare function js_{{crate_name}}_hello"));
        assert!(TPL_INDEX_TS.contains("return js_{{crate_name}}_hello(name);"));
    }
}
