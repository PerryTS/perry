//! `perry native` — tooling for native-bindings packages.
//!
//! Phase 3 of the native package ecosystem (issue #466). The
//! commands here drop the barrier to writing a wrapper from
//! "hours of archaeology" to "minutes of `init`+`validate`".

use clap::{Args, Subcommand};

mod init;
mod list;
mod validate;

#[derive(Args, Debug)]
pub struct NativeArgs {
    #[command(subcommand)]
    pub command: NativeCommand,
}

#[derive(Subcommand, Debug)]
pub enum NativeCommand {
    /// Scaffold a new native-bindings package.
    ///
    /// Creates a directory with package.json (perry.nativeLibrary
    /// manifest), src/index.ts (TypeScript surface), Cargo.toml,
    /// src/lib.rs (one example #[no_mangle] entry), README, LICENSE,
    /// and a release.yml that prebuilds staticlibs for every
    /// supported target on tag.
    Init(init::InitArgs),

    /// Validate the wrapper in the current directory.
    ///
    /// Parses the manifest, runs `cargo build --release`, diffs the
    /// resulting `.a`'s exported symbols against the `functions[]`
    /// array, and reports drift with file:line references.
    Validate(validate::ValidateArgs),

    /// List the well-known bindings shipped with this Perry build.
    List(list::ListArgs),
}

pub fn run(args: NativeArgs, format: crate::OutputFormat, use_color: bool) -> anyhow::Result<()> {
    match args.command {
        NativeCommand::Init(a) => init::run(a, format, use_color),
        NativeCommand::Validate(a) => validate::run(a, format, use_color),
        NativeCommand::List(a) => list::run(a, format, use_color),
    }
}
