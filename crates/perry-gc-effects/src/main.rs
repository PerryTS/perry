use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use perry_gc_effects::{
    analyze, compare, load_rules, read_table, workspace_root, Class, Classification, Mode, SEEDS,
};
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
    Check {
        #[arg(long)]
        target: String,
        #[arg(long)]
        table: PathBuf,
        #[arg(long)]
        write_fresh: Option<PathBuf>,
        #[arg(long)]
        allow_safe_drift: bool,
        #[arg(long, default_value_t = 1000)]
        min_symbols: usize,
        #[arg(required = true)]
        archives: Vec<PathBuf>,
    },
    Why {
        #[arg(long = "archive", required = true)]
        archives: Vec<PathBuf>,
        #[arg(required = true)]
        symbols: Vec<String>,
    },
    Lint {
        #[arg(long, default_value_t = 1000)]
        min_symbols: usize,
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
        Command::Check {
            target,
            table,
            write_fresh,
            allow_safe_drift,
            min_symbols,
            archives,
        } => {
            let (g, res) = analyze(&archives, &rules, &root)?;
            stats(&res);
            if res.table.len() < min_symbols {
                bail!(
                    "only {} exported symbols (< {min_symbols})",
                    res.table.len()
                );
            }
            if let Some(out) = write_fresh {
                perry_gc_effects::write_table(&res.table, &target, &archives, &out)?;
            }
            let committed = read_table(&table)?;
            let (unsafe_drift, safe) = compare(&committed, &res.table);
            if !unsafe_drift.is_empty() {
                eprintln!("UNSAFE: {} changes", unsafe_drift.len());
                for d in unsafe_drift.iter().take(60) {
                    eprintln!(
                        "  {}: committed {}, archives say {}",
                        d.name, d.committed, d.fresh
                    );
                    let mode = if d.throws {
                        Mode::Throws
                    } else {
                        match d.committed.as_str() {
                            "Leaf" => Mode::L2,
                            "AllocOnly" => Mode::L2b,
                            _ => Mode::L2bThrow,
                        }
                    };
                    for step in res.witness_path(&g, &d.name, mode).iter().take(12) {
                        eprintln!("      {step}");
                    }
                }
            }
            if !safe.is_empty() {
                eprintln!(
                    "::{}::DRIFT: {} safe changes",
                    if allow_safe_drift { "warning" } else { "error" },
                    safe.len()
                );
                for d in safe.iter().take(40) {
                    eprintln!(
                        "  {}: committed {}, archives say {}",
                        d.name, d.committed, d.fresh
                    );
                }
            }
            if !unsafe_drift.is_empty() || !safe.is_empty() && !allow_safe_drift {
                bail!("table differs from archives");
            }
            eprintln!(
                "[gc-call-effects] {}: no UNSAFE drift ({} safe)",
                table.display(),
                safe.len()
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
        Command::Lint { min_symbols } => {
            for kind in ["collector", "poll", "js", "throw", "panic", "extern"] {
                if !rules.iter().any(|r| r.kind == kind) {
                    bail!("seeds.txt has no {kind} rule");
                }
            }
            for target in ["linux-x86_64", "macos-aarch64", "windows-x86_64"] {
                let path = root.join(format!("crates/perry-codegen/src/gc_effects/{target}.tsv"));
                let text = std::fs::read_to_string(&path)?;
                let table = read_table(&path)?;
                if !text.contains(&format!("# target: {target}\n"))
                    || table.len() < min_symbols
                    || !table.values().any(|e| e.class == Class::Leaf)
                    || !table.values().any(|e| !e.throws)
                {
                    bail!("invalid/vacuous {}", path.display());
                }
                eprintln!("[gc-call-effects] {target}: {} symbols", table.len());
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
