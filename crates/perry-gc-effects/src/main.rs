use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use perry_gc_effects::{analyze, load_rules, workspace_root, Classification, Mode, SEEDS};
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Derive GC and unwind effects from runtime machine code")]
struct Cli {
    #[arg(long, global = true)]
    rules: Option<PathBuf>,
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Generate {
        #[arg(long)]
        target: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 1000)]
        min_symbols: usize,
        #[arg(required = true)]
        archives: Vec<PathBuf>,
    },
    /// Write `<runtime archive>.gc-effects` beside every runtime archive in a
    /// directory: the runtime alone, and the runtime with its stdlib.
    Sidecars {
        /// Override the `<os>-<arch>` label detected from the archives.
        #[arg(long)]
        target: Option<String>,
        #[arg(long, default_value_t = 1000)]
        min_symbols: usize,
        lib_dir: PathBuf,
    },
    Why {
        #[arg(long = "archive", required = true)]
        archives: Vec<PathBuf>,
        #[arg(required = true)]
        symbols: Vec<String>,
    },
}
fn stats(res: &Classification) {
    let s = &res.stats;
    let mut classes = std::collections::BTreeMap::new();
    let mut throws = std::collections::BTreeMap::new();
    for e in res.table.values() {
        *classes.entry(e.class.to_string()).or_insert(0) += 1;
        *throws.entry(e.throws_name()).or_insert(0) += 1;
    }
    eprintln!("[gc-call-effects] nodes={} edges={} indirect_call_nodes={} indirect_jump_nodes={} exempted_indirect={} trusted_indirect={} named_seeds={:?}",s.nodes,s.edges,s.indirect_call_nodes,s.indirect_jump_nodes,s.exempted_indirect_nodes,s.trusted_indirect_nodes,s.named_seed_nodes);
    eprintln!("[gc-call-effects] classes={classes:?} throws={throws:?} unresolved_externs={} shadowed_demoted={}",s.unresolved_externs.len(),s.shadowed_demoted.len());
    if !s.unused_exemptions.is_empty() {
        eprintln!(
            "[gc-call-effects] exemptions unused on this target:\n  {}",
            s.unused_exemptions.join("\n  ")
        );
    }
}
fn run() -> Result<()> {
    let cli = Cli::parse();
    let root = cli.root.unwrap_or_else(workspace_root);
    let text = cli
        .rules
        .as_ref()
        .map(std::fs::read_to_string)
        .transpose()?
        .unwrap_or_else(|| SEEDS.into());
    let rules = load_rules(&text)?;
    match cli.command {
        Command::Generate {
            target,
            out,
            min_symbols,
            archives,
        } => {
            let (_, res) = analyze(&archives, &rules, &root)?;
            stats(&res);
            if res.table.len() < min_symbols {
                bail!(
                    "only {} exported symbols (< {min_symbols})",
                    res.table.len()
                );
            }
            perry_gc_effects::write_table(&res.table, &target, &archives, &out)?;
            eprintln!(
                "[gc-call-effects] wrote {} ({} symbols)",
                out.display(),
                res.table.len()
            );
        }
        Command::Sidecars {
            target,
            min_symbols,
            lib_dir,
        } => {
            let commit = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["rev-parse", "--short=12", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                .unwrap_or_else(|| "unknown".into());
            let analyzer = format!("{} {commit}", env!("CARGO_PKG_VERSION"));
            let t = std::time::Instant::now();
            let sidecars = perry_gc_effects::sidecar::produce(
                &lib_dir,
                &rules,
                &root,
                &analyzer,
                target.as_deref(),
                min_symbols,
                &|name, res, secs| {
                    eprintln!("[gc-call-effects] {name}: {secs:.1}s");
                    stats(res);
                },
            )?;
            perry_gc_effects::sidecar::write_sidecars(&sidecars)?;
            for (path, s) in &sidecars {
                eprintln!(
                    "[gc-call-effects] wrote {} ({} combinations)",
                    path.display(),
                    s.combinations.len()
                );
            }
            eprintln!(
                "[gc-call-effects] sidecars: {:.1}s total",
                t.elapsed().as_secs_f64()
            );
        }
        Command::Why { archives, symbols } => {
            let (g, res) = analyze(&archives, &rules, &root)?;
            for name in symbols {
                println!(
                    "== {name}: {}",
                    res.table
                        .get(&name)
                        .map(|e| e.class.to_string())
                        .unwrap_or_else(|| "not exported".into())
                );
                for mode in Mode::ALL {
                    println!("  [{}]", mode.name());
                    for step in res.witness_path(&g, &name, mode) {
                        println!("    {step}");
                    }
                }
            }
        }
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("perry-gc-effects: {e:#}");
        std::process::exit(1);
    }
}
