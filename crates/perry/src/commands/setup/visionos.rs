use anyhow::{Context, Result};
use console::style;
use dialoguer::{Confirm, Input};

use super::super::publish::{config_path, AppleSavedConfig, PerryConfig};

use super::*;

pub fn visionos_wizard(saved: &mut PerryConfig) -> Result<()> {
    println!("  {}", style("visionOS Setup").bold());
    println!();

    let existing_apple = saved.apple.clone().unwrap_or_default();

    println!(
        "  {} App Store Connect API Key",
        style("Step 1/2 —").cyan().bold()
    );
    println!();

    let has_existing = existing_apple.p8_key_path.is_some()
        && existing_apple.key_id.is_some()
        && existing_apple.issuer_id.is_some();

    let (p8_path, key_id, issuer_id, team_id) = if has_existing {
        let p8 = existing_apple.p8_key_path.clone().unwrap();
        let kid = existing_apple.key_id.clone().unwrap();
        let iss = existing_apple.issuer_id.clone().unwrap();
        let tid = existing_apple.team_id.clone().unwrap_or_default();
        println!("  Found existing credentials (shared with iOS/macOS):");
        println!("    Key ID:    {}", style(&kid).bold());
        println!("    Issuer ID: {}", style(&iss).dim());
        println!("    .p8 key:   {}", style(&p8).dim());
        if !tid.is_empty() {
            println!("    Team ID:   {}", style(&tid).dim());
        }
        println!();
        let reuse = Confirm::new()
            .with_prompt("  Use these existing credentials?")
            .default(true)
            .interact()?;
        if reuse {
            (p8, kid, iss, tid)
        } else {
            prompt_api_credentials()?
        }
    } else {
        println!("  You need an App Store Connect API key.");
        println!(
            "  1. Go to: {}",
            style("https://appstoreconnect.apple.com/access/integrations/api").underlined()
        );
        println!("  2. Create an API key with \"App Manager\" or \"Admin\" role.");
        println!("  3. Download the .p8 file and note the Key ID and Issuer ID.");
        println!();
        prompt_api_credentials()?
    };

    saved.apple = Some(AppleSavedConfig {
        p8_key_path: Some(p8_path),
        key_id: Some(key_id),
        issuer_id: Some(issuer_id),
        team_id: if team_id.is_empty() {
            None
        } else {
            Some(team_id)
        },
        ..existing_apple
    });

    println!();
    println!("  {} Bundle ID", style("Step 2/2 —").cyan().bold());
    println!();

    let perry_toml_path = std::env::current_dir()?.join("perry.toml");
    let existing_bid = if perry_toml_path.exists() {
        let content = std::fs::read_to_string(&perry_toml_path)?;
        let parsed: toml::Table = content.parse().unwrap_or_default();
        parsed
            .get("visionos")
            .and_then(|w| w.get("bundle_id"))
            .and_then(|v| v.as_str())
            .or_else(|| {
                parsed
                    .get("app")
                    .and_then(|a| a.get("bundle_id"))
                    .and_then(|v| v.as_str())
            })
            .or_else(|| {
                parsed
                    .get("project")
                    .and_then(|p| p.get("bundle_id"))
                    .and_then(|v| v.as_str())
            })
            .or_else(|| {
                parsed
                    .get("ios")
                    .and_then(|p| p.get("bundle_id"))
                    .and_then(|v| v.as_str())
            })
            .map(|s| s.to_string())
    } else {
        None
    };

    let bundle_id: String = if let Some(ref bid) = existing_bid {
        println!("  Found existing bundle ID: {}", style(bid).bold());
        let reuse = Confirm::new()
            .with_prompt("  Use this bundle ID?")
            .default(true)
            .interact()?;
        if reuse {
            bid.clone()
        } else {
            Input::new()
                .with_prompt("  visionOS Bundle ID (e.g. com.example.myvision)")
                .interact_text()?
        }
    } else {
        Input::new()
            .with_prompt("  visionOS Bundle ID (e.g. com.example.myvision)")
            .interact_text()?
    };

    if !perry_toml_path.exists() {
        std::fs::write(&perry_toml_path, "")?;
    }
    save_visionos_bundle_id(&perry_toml_path, &bundle_id)?;

    println!();
    println!("  {} visionOS setup complete!", style("✓").green().bold());
    println!();
    println!("  Saved to:");
    println!("    Global: {}", style(config_path().display()).dim());
    println!("    Project: {}", style(perry_toml_path.display()).dim());
    println!();
    println!("  Next steps:");
    println!("    perry compile app.ts --target visionos-simulator");
    println!("    perry run visionos");
    println!();

    Ok(())
}

pub fn save_visionos_bundle_id(perry_toml_path: &std::path::Path, bundle_id: &str) -> Result<()> {
    let content = if perry_toml_path.exists() {
        std::fs::read_to_string(perry_toml_path)?
    } else {
        String::new()
    };
    let mut doc = content
        .parse::<toml::Table>()
        .unwrap_or_else(|_| toml::Table::new());

    let visionos = doc
        .entry("visionos")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[visionos] in perry.toml is not a table"))?;

    visionos.insert("bundle_id".into(), toml::Value::String(bundle_id.into()));

    let new_content = toml::to_string_pretty(&doc).context("Failed to serialize perry.toml")?;
    std::fs::write(perry_toml_path, new_content)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// watchOS wizard
// ---------------------------------------------------------------------------
