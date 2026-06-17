use crate::compose::ComposeEngine;
use crate::config::ProjectConfig;
use crate::error::Result;
use crate::project::ComposeProject;
use clap::{Args, Parser, Subcommand};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(
    name = "perry-compose",
    version,
    about = "Docker Compose-like CLI for container backends"
)]
pub struct Cli {
    #[arg(short = 'f', long = "file", value_name = "FILE", global = true)]
    pub files: Vec<PathBuf>,

    #[arg(short = 'p', long = "project-name", global = true)]
    pub project_name: Option<String>,

    #[arg(long = "env-file", value_name = "FILE", global = true)]
    pub env_files: Vec<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start services
    Up(UpArgs),
    /// Stop and remove services
    Down(DownArgs),
    /// Start existing stopped services
    Start(ServiceArgs),
    /// Stop running services
    Stop(ServiceArgs),
    /// Restart services
    Restart(ServiceArgs),
    /// List service status
    Ps(PsArgs),
    /// View output from containers
    Logs(LogsArgs),
    /// Execute a command in a running service
    Exec(ExecArgs),
    /// Validate and view the Compose configuration
    Config(ConfigArgs),
}

#[derive(Args, Debug)]
pub struct UpArgs {
    #[arg(short = 'd', long = "detach")]
    pub detach: bool,
    #[arg(long = "build")]
    pub build: bool,
    #[arg(long = "remove-orphans")]
    pub remove_orphans: bool,
    pub services: Vec<String>,
}

#[derive(Args, Debug)]
pub struct DownArgs {
    #[arg(short = 'v', long = "volumes")]
    pub volumes: bool,
    #[arg(long = "remove-orphans")]
    pub remove_orphans: bool,
    pub services: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ServiceArgs {
    pub services: Vec<String>,
}

#[derive(Args, Debug)]
pub struct PsArgs {
    #[arg(short = 'a', long = "all")]
    pub all: bool,
    pub services: Vec<String>,
}

#[derive(Args, Debug)]
pub struct LogsArgs {
    #[arg(short = 'f', long = "follow")]
    pub follow: bool,
    #[arg(long = "tail")]
    pub tail: Option<u32>,
    #[arg(short = 't', long = "timestamps")]
    pub timestamps: bool,
    pub services: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ExecArgs {
    pub service: String,
    #[arg(trailing_var_arg = true)]
    pub cmd: Vec<String>,
    #[arg(short = 'u', long = "user")]
    pub user: Option<String>,
    #[arg(short = 'w', long = "workdir")]
    pub workdir: Option<String>,
    #[arg(short = 'e', long = "env")]
    pub env: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[arg(long = "format", default_value = "yaml")]
    pub format: String,
    #[arg(long = "resolve-image-digests")]
    pub resolve: bool,
}

pub async fn run(cli: Cli) -> Result<()> {
    let config = ProjectConfig::new(
        cli.files.clone(),
        cli.project_name.clone(),
        cli.env_files.clone(),
    );

    let project = ComposeProject::load(&config)?;

    let backend = crate::backend::detect_backend().await?;
    let backend = Arc::from(backend);

    let engine = Arc::new(ComposeEngine::new(
        project.spec.clone(),
        project.project_name.clone(),
        backend,
    ));

    match cli.command {
        Commands::Up(args) => {
            engine
                .up(&args.services, args.detach, args.build, args.remove_orphans)
                .await?;
        }
        Commands::Down(args) => {
            engine
                .down(&args.services, args.remove_orphans, args.volumes)
                .await?;
        }
        Commands::Start(args) => {
            engine.start(&args.services).await?;
        }
        Commands::Stop(args) => {
            engine.stop(&args.services).await?;
        }
        Commands::Restart(args) => {
            engine.restart(&args.services).await?;
        }
        Commands::Ps(_args) => {
            let infos = engine.ps().await?;
            print_ps_table(&infos);
        }
        Commands::Logs(args) => {
            let logs_map = engine.logs(&args.services, args.tail).await?;
            let mut names: Vec<&String> = logs_map.keys().collect();
            names.sort();
            for name in names {
                let log = &logs_map[name];
                for line in log.lines() {
                    println!("{:<12} | {}", name, line);
                }
            }
        }
        Commands::Exec(args) => {
            let mut env_map = HashMap::new();
            for e in args.env {
                if let Some((k, v)) = e.split_once('=') {
                    env_map.insert(k.to_string(), v.to_string());
                }
            }
            let env = if env_map.is_empty() {
                None
            } else {
                Some(env_map)
            };
            let logs = engine
                .exec(
                    &args.service,
                    &args.cmd,
                    env.as_ref(),
                    args.workdir.as_deref(),
                )
                .await?;
            print!("{}", logs.stdout);
            eprint!("{}", logs.stderr);
        }
        Commands::Config(args) => {
            let yaml = engine.config()?;
            if args.format == "json" {
                let value: serde_yaml::Value = serde_yaml::from_str(&yaml)?;
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!("{}", yaml);
            }
        }
    }

    Ok(())
}

fn print_ps_table(infos: &[crate::types::ContainerInfo]) {
    println!("{:<24}  {:<12}  {:<36}", "SERVICE", "STATUS", "CONTAINER");
    println!("{}", "-".repeat(76));
    for info in infos {
        println!("{:<24}  {:<12}  {:<36}", info.name, info.status, info.id);
    }
}
