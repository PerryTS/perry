//! CLI entry point for `perry-compose` binary.

use clap::Parser;
use perry_container_compose::cli::{run, Cli};
use tracing_subscriber::{fmt, EnvFilter};

fn main() {
    // Initialise tracing (RUST_LOG env controls verbosity)
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let cli = Cli::parse();

    // The engine is executor-agnostic async code; `rt::block_on` drives it on
    // a turnloop loop owned by this thread (see `perry_container_compose::rt`).
    if let Err(e) = perry_container_compose::rt::block_on(run(cli)) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
