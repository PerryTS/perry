//! Init command - initialize a new perry project

use anyhow::Result;
use clap::Args;
use std::fs;
use std::path::{Path, PathBuf};

use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Project directory (default: current)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Project name (defaults to directory name)
    #[arg(long)]
    pub name: Option<String>,
}

const DEFAULT_MAIN_TS: &str = r#"// Main entry point

function main(): void {
    console.log("Hello from Perry!");
}

main();
"#;

const DEFAULT_CONFIG: &str = r#"# Perry configuration
# https://github.com/PerryTS/perry

[project]
name = "{name}"
entry = "src/main.ts"

[build]
out_dir = "dist"
opt_level = 2
"#;

// package.json carries the npm-interop layer. Notably it is the *only* home for
// `perry.compilePackages` (the list of npm packages to compile natively) — that
// setting is read exclusively from here, never from perry.toml. We seed it empty
// so the field is discoverable. `private: true` keeps tooling from treating the
// scaffold as a publishable package, and `main` mirrors perry.toml's `entry`.
// `{name}` is substituted with a JSON-escaped, already-quoted string (see `run`)
// so a project name containing `"` or `\` still produces a parseable manifest.
const DEFAULT_PACKAGE_JSON: &str = r#"{
  "name": {name},
  "version": "0.1.0",
  "private": true,
  "main": "src/main.ts",
  "perry": {
    "compilePackages": []
  }
}
"#;

const DEFAULT_GITIGNORE: &str = r#"# Perry build outputs
dist/
*.o

# Generated type stubs (regenerate with `perry types`)
.perry/types/

# Node modules
node_modules/

# IDE
.vscode/
.idea/
"#;

const DEFAULT_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2023",
    "module": "preserve",
    "moduleResolution": "bundler",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true,
    "baseUrl": ".",
    "paths": {
{paths}
    }
  },
  "include": ["src", ".perry/types/stdlib/index.d.ts"]
}
"#;

/// Read `perry.packageAliases` (npm package name → replacement package name)
/// from the project's package.json, so the generated tsconfig can mirror what
/// the Perry compiler resolves. Returns sorted (from, to) pairs.
fn package_aliases(project_path: &Path) -> Vec<(String, String)> {
    let pkg_path = project_path.join("package.json");
    let Ok(content) = fs::read_to_string(&pkg_path) else {
        return Vec::new();
    };
    let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    let Some(aliases) = pkg
        .get("perry")
        .and_then(|p| p.get("packageAliases"))
        .and_then(|a| a.as_object())
    else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = aliases
        .iter()
        .filter_map(|(from, to)| to.as_str().map(|to| (from.clone(), to.to_string())))
        .collect();
    out.sort();
    out
}

/// Build the `compilerOptions.paths` map the IDE's tsc needs to resolve the
/// same module specifiers Perry resolves at compile time: the always-present
/// `perry/*` type-stub mapping, plus one bare + one wildcard entry per
/// `perry.packageAliases` alias (`"<from>": ["./node_modules/<to>"]`).
fn tsconfig_paths(aliases: &[(String, String)]) -> serde_json::Map<String, serde_json::Value> {
    let mut paths = serde_json::Map::new();
    paths.insert(
        "perry/*".to_string(),
        serde_json::json!(["./.perry/types/perry/*/index.d.ts"]),
    );
    for (from, to) in aliases {
        paths.insert(
            from.clone(),
            serde_json::json!([format!("./node_modules/{to}")]),
        );
        paths.insert(
            format!("{from}/*"),
            serde_json::json!([format!("./node_modules/{to}/*")]),
        );
    }
    paths
}

/// Render the `paths` entries indented to sit inside the DEFAULT_TSCONFIG
/// template's `"paths": { … }` block (6-space indent, comma-separated).
fn render_paths_block(paths: &serde_json::Map<String, serde_json::Value>) -> String {
    paths
        .iter()
        .map(|(k, v)| {
            format!(
                "      {}: {}",
                serde_json::to_string(k).unwrap_or_else(|_| format!("\"{k}\"")),
                serde_json::to_string(v).unwrap_or_else(|_| "[]".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join(",\n")
}

/// Adjust root-relative generated path targets for an existing TypeScript
/// `baseUrl`. TypeScript resolves every `paths` target from `baseUrl`, so a
/// config rooted at `src` needs `../node_modules/...` and `../.perry/...`.
fn paths_for_base_url(
    paths: &serde_json::Map<String, serde_json::Value>,
    base_url: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let mut depth = 0usize;
    for component in Path::new(base_url).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(_) => depth += 1,
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => return None,
        }
    }
    if depth == 0 {
        return Some(paths.clone());
    }

    let prefix = "../".repeat(depth);
    let mut adjusted = serde_json::Map::new();
    for (key, targets) in paths {
        let targets = targets.as_array()?;
        let mut values = Vec::with_capacity(targets.len());
        for target in targets {
            let target = target.as_str()?;
            values.push(serde_json::Value::String(format!(
                "{prefix}{}",
                target.strip_prefix("./").unwrap_or(target)
            )));
        }
        adjusted.insert(key.clone(), serde_json::Value::Array(values));
    }
    Some(adjusted)
}

/// Merge alias paths into an existing tsconfig.json. Returns the rewritten
/// contents if a change is needed and the file parses as JSON, `Ok(None)` if
/// nothing changed, or `Err` with the block to paste if it can't be parsed
/// (e.g. JSONC with comments/trailing commas).
fn merge_paths_into_existing(
    existing: &str,
    paths: &serde_json::Map<String, serde_json::Value>,
) -> std::result::Result<Option<String>, String> {
    let mut root: serde_json::Value =
        serde_json::from_str(existing).map_err(|_| render_paths_block(paths))?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| render_paths_block(paths))?;
    let co = obj
        .entry("compilerOptions")
        .or_insert_with(|| serde_json::json!({}));
    let co = co
        .as_object_mut()
        .ok_or_else(|| render_paths_block(paths))?;
    let base_url = match co.get("baseUrl") {
        Some(value) => value
            .as_str()
            .ok_or_else(|| render_paths_block(paths))?
            .to_string(),
        None => {
            co.insert("baseUrl".to_string(), serde_json::json!("."));
            ".".to_string()
        }
    };
    let paths = paths_for_base_url(paths, &base_url).ok_or_else(|| render_paths_block(paths))?;
    let existing_paths = co.entry("paths").or_insert_with(|| serde_json::json!({}));
    let existing_paths = existing_paths
        .as_object_mut()
        .ok_or_else(|| render_paths_block(&paths))?;
    let mut changed = false;
    for (k, v) in &paths {
        if existing_paths.get(k) != Some(v) {
            existing_paths.insert(k.clone(), v.clone());
            changed = true;
        }
    }
    if !changed {
        return Ok(None);
    }
    let mut out = serde_json::to_string_pretty(&root).map_err(|_| render_paths_block(&paths))?;
    out.push('\n');
    Ok(Some(out))
}

pub fn run(args: InitArgs, format: OutputFormat, _use_color: bool) -> Result<()> {
    let project_path = args.path.canonicalize().unwrap_or(args.path.clone());

    // Determine project name
    let name = args.name.unwrap_or_else(|| {
        project_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("my-project")
            .to_string()
    });

    // Create directories
    let src_dir = project_path.join("src");
    fs::create_dir_all(&src_dir)?;

    match format {
        OutputFormat::Text => println!("Creating new perry project '{}'...\n", name),
        OutputFormat::Json => {}
    }

    // Create perry.toml
    let config_path = project_path.join("perry.toml");
    if !config_path.exists() {
        let config_content = DEFAULT_CONFIG.replace("{name}", &name);
        fs::write(&config_path, config_content)?;
        match format {
            OutputFormat::Text => println!("  Created perry.toml"),
            OutputFormat::Json => {}
        }
    } else {
        match format {
            OutputFormat::Text => println!("  Skipped perry.toml (already exists)"),
            OutputFormat::Json => {}
        }
    }

    // Create package.json (npm-interop layer; sole home for perry.compilePackages)
    let package_json_path = project_path.join("package.json");
    if !package_json_path.exists() {
        // Escape via serde so names with `"`/`\`/control chars stay valid JSON.
        // `to_string` returns the value already wrapped in quotes.
        let name_json = serde_json::to_string(&name)?;
        let package_json_content = DEFAULT_PACKAGE_JSON.replace("{name}", &name_json);
        fs::write(&package_json_path, package_json_content)?;
        match format {
            OutputFormat::Text => println!("  Created package.json"),
            OutputFormat::Json => {}
        }
    } else {
        match format {
            OutputFormat::Text => println!("  Skipped package.json (already exists)"),
            OutputFormat::Json => {}
        }
    }

    // Create src/main.ts
    let main_path = src_dir.join("main.ts");
    if !main_path.exists() {
        fs::write(&main_path, DEFAULT_MAIN_TS)?;
        match format {
            OutputFormat::Text => println!("  Created src/main.ts"),
            OutputFormat::Json => {}
        }
    } else {
        match format {
            OutputFormat::Text => println!("  Skipped src/main.ts (already exists)"),
            OutputFormat::Json => {}
        }
    }

    // Create .gitignore
    let gitignore_path = project_path.join(".gitignore");
    if !gitignore_path.exists() {
        fs::write(&gitignore_path, DEFAULT_GITIGNORE)?;
        match format {
            OutputFormat::Text => println!("  Created .gitignore"),
            OutputFormat::Json => {}
        }
    } else {
        match format {
            OutputFormat::Text => println!("  Skipped .gitignore (already exists)"),
            OutputFormat::Json => {}
        }
    }

    // Create / update tsconfig.json. The `paths` block mirrors
    // `perry.packageAliases` from package.json so the IDE's tsc language
    // server resolves aliased imports to the same target Perry uses at
    // compile time (perry.packageAliases is otherwise compiler-only).
    let aliases = package_aliases(&project_path);
    let paths = tsconfig_paths(&aliases);
    let tsconfig_path = project_path.join("tsconfig.json");
    if !tsconfig_path.exists() {
        let contents = DEFAULT_TSCONFIG.replace("{paths}", &render_paths_block(&paths));
        fs::write(&tsconfig_path, contents)?;
        match format {
            OutputFormat::Text => {
                println!("  Created tsconfig.json");
                if !aliases.is_empty() {
                    println!(
                        "    + {} packageAliases path(s) for IDE resolution",
                        aliases.len()
                    );
                }
            }
            OutputFormat::Json => {}
        }
    } else {
        // Always sync the built-in `perry/*` path, even when the project has no
        // package aliases. Alias entries, when present, are merged alongside it.
        let existing = fs::read_to_string(&tsconfig_path)?;
        match merge_paths_into_existing(&existing, &paths) {
            Ok(Some(updated)) => {
                fs::write(&tsconfig_path, updated)?;
                if let OutputFormat::Text = format {
                    println!("  Updated tsconfig.json (synced Perry paths)");
                }
            }
            Ok(None) => {
                if let OutputFormat::Text = format {
                    println!("  Skipped tsconfig.json (Perry paths already in sync)");
                }
            }
            Err(block) => {
                if let OutputFormat::Text = format {
                    println!(
                        "  Skipped tsconfig.json (couldn't auto-merge — not plain JSON).\n\
                         \x20   Add these to compilerOptions.paths so the IDE matches Perry:\n{block}"
                    );
                }
            }
        }
    }

    // Write Perry type stubs into .perry/types/perry/
    super::types::write_perry_type_stubs(&project_path, !matches!(format, OutputFormat::Text))?;

    match format {
        OutputFormat::Text => {
            println!("\nDone! Next steps:");
            println!("  cd {}", project_path.display());
            println!("  perry compile src/main.ts");
            println!("  ./main");
        }
        OutputFormat::Json => {
            let result = serde_json::json!({
                "success": true,
                "project_name": name,
                "path": project_path.to_string_lossy(),
            });
            println!("{}", serde_json::to_string(&result)?);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_base_url_rebases_generated_paths() {
        let paths = tsconfig_paths(&[("electron".to_string(), "@perryts/electron".to_string())]);
        let existing = r#"{"compilerOptions":{"baseUrl":"src","paths":{}}}"#;
        let updated = merge_paths_into_existing(existing, &paths)
            .expect("merge")
            .expect("changed");
        let value: serde_json::Value = serde_json::from_str(&updated).expect("json");
        let paths = &value["compilerOptions"]["paths"];
        assert_eq!(paths["perry/*"][0], "../.perry/types/perry/*/index.d.ts");
        assert_eq!(paths["electron"][0], "../node_modules/@perryts/electron");
    }

    #[test]
    fn built_in_perry_path_is_merged_without_package_aliases() {
        let paths = tsconfig_paths(&[]);
        let existing = r#"{"compilerOptions":{"baseUrl":".","paths":{}}}"#;
        let updated = merge_paths_into_existing(existing, &paths)
            .expect("merge")
            .expect("changed");
        assert!(updated.contains(".perry/types/perry/*/index.d.ts"));
    }
}
