//! Init command - initialize a new perry project

use anyhow::Result;
use clap::Args;
use std::fs;
use std::path::PathBuf;

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
    "paths": {
      "perry/*": ["./.perry/types/perry/*/index.d.ts"]
    }
  },
  "include": ["src", ".perry/types/stdlib/index.d.ts"]
}
"#;

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

    // Create tsconfig.json
    let tsconfig_path = project_path.join("tsconfig.json");
    if !tsconfig_path.exists() {
        fs::write(&tsconfig_path, DEFAULT_TSCONFIG)?;
        match format {
            OutputFormat::Text => println!("  Created tsconfig.json"),
            OutputFormat::Json => {}
        }
    } else {
        match format {
            OutputFormat::Text => println!("  Skipped tsconfig.json (already exists)"),
            OutputFormat::Json => {}
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
