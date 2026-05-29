//! Login command - authenticate with Perry via GitHub OAuth device flow

use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;
use serde::Deserialize;
use std::io::Write;

use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct LoginArgs {
    /// Dashboard server URL
    #[arg(long, default_value = "https://app.perryts.com")]
    pub server: Option<String>,
}

// #854: deserialized OAuth start response; `ok` is part of the wire shape
// but the client only consumes `authorize_url`.
#[allow(dead_code)]
#[derive(Deserialize)]
struct StartResponse {
    ok: bool,
    authorize_url: String,
}

#[derive(Deserialize)]
struct PollResponse {
    authorized: bool,
    api_token: Option<String>,
    github_username: Option<String>,
    tier: Option<String>,
}

pub fn run(args: LoginArgs, format: OutputFormat, use_color: bool) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to create async runtime")?;
    rt.block_on(run_async(args, format, use_color))
}

async fn run_async(args: LoginArgs, format: OutputFormat, _use_color: bool) -> Result<()> {
    let server_url = args.server.as_deref().unwrap_or("https://app.perryts.com");

    // Check if already logged in
    let saved = super::publish::load_config();
    if let Some(ref token) = saved.api_token {
        if let Some(ref username) = saved.github_username {
            if let OutputFormat::Text = format {
                println!(
                    "  {} Already logged in as {}",
                    style("✓").green().bold(),
                    style(format!("@{}", username)).bold()
                );
                println!("  To log in as a different user, run: perry logout");
            }
            return Ok(());
        }
        // Has token but no username — re-validate or proceed
        let _ = token;
    }

    // Generate a random device code
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let device_code = format!("{:012X}", ts % 0xFFFF_FFFF_FFFF);

    if let OutputFormat::Text = format {
        println!();
        println!("  {} Logging in to Perry", style("→").cyan().bold());
        println!();
    }

    // Register device code with dashboard
    let client = reqwest::Client::new();
    let start_resp = client
        .post(format!("{}/api/cli/start", server_url))
        .json(&serde_json::json!({ "device_code": device_code }))
        .send()
        .await
        .context("Failed to connect to dashboard")?;

    if !start_resp.status().is_success() {
        let body = start_resp.text().await.unwrap_or_default();
        bail!("Failed to start login: {}", body);
    }

    let start: StartResponse = start_resp.json().await.context("Invalid response")?;
    let authorize_url = start.authorize_url;

    // Open browser
    if let OutputFormat::Text = format {
        println!("  Opening browser for GitHub sign-in...");
        println!();
    }

    let open_result = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .arg(&authorize_url)
            .spawn()
    } else if cfg!(target_os = "linux") {
        std::process::Command::new("xdg-open")
            .arg(&authorize_url)
            .spawn()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/c", "start", &authorize_url])
            .spawn()
    } else {
        Err(std::io::Error::other("unsupported platform"))
    };

    if open_result.is_err() {
        if let OutputFormat::Text = format {
            println!(
                "  {} Could not open browser. Visit this URL manually:",
                style("!").yellow()
            );
            println!("  {}", style(&authorize_url).underlined());
            println!();
        }
    }

    if let OutputFormat::Text = format {
        print!("  Waiting for authorization...");
        std::io::stdout().flush().ok();
    }

    // Poll for authorization
    let mut attempts = 0;
    let max_attempts = 150; // 5 minutes at 2s intervals
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        attempts += 1;

        if attempts > max_attempts {
            println!();
            bail!("Login timed out. Please try again.");
        }

        let poll_resp = client
            .get(format!("{}/api/cli/poll?code={}", server_url, device_code))
            .send()
            .await;

        let poll_resp = match poll_resp {
            Ok(r) => r,
            Err(_) => continue, // network hiccup, retry
        };

        if !poll_resp.status().is_success() {
            continue;
        }

        let poll: PollResponse = match poll_resp.json().await {
            Ok(p) => p,
            Err(_) => continue,
        };

        if poll.authorized {
            let api_token = poll.api_token.unwrap_or_default();
            let github_username = poll.github_username.unwrap_or_default();
            let tier = poll.tier.unwrap_or_else(|| "free".to_string());

            // Save to config
            let mut config = super::publish::load_config();
            config.api_token = Some(api_token);
            config.github_username = Some(github_username.clone());
            super::publish::save_config(&config).ok();

            if let OutputFormat::Text = format {
                println!(" {}", style("done").green());
                println!();
                println!(
                    "  {} Logged in as {} ({})",
                    style("✓").green().bold(),
                    style(format!("@{}", github_username)).bold(),
                    tier
                );
                println!();
            }

            return Ok(());
        }

        // Print a dot to show progress
        if attempts % 5 == 0 {
            if let OutputFormat::Text = format {
                print!(".");
                std::io::stdout().flush().ok();
            }
        }
    }
}
