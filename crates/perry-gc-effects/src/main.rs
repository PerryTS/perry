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
    /// Native throw origins and one shortest open witness per exported entry.
    ThrowCensus {
        #[arg(long)]
        out: PathBuf,
        #[arg(long = "archive", required = true)]
        archives: Vec<PathBuf>,
    },
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
    /// Check that every realm native whose body can reach a byte-cell birth
    /// is an entry of the typed-array family or declares the birth
    /// (`family_reach`'s "never" rests on it); print the census.
    Births {
        #[arg(long)]
        manifest: PathBuf,
        /// Replace the built-in rules (`scripts/gc_call_effects/byte_cell_births.txt`).
        #[arg(long = "birth-rules")]
        birth_rules: Option<PathBuf>,
        /// Per-native census TSV.
        #[arg(long)]
        census: Option<PathBuf>,
        /// The functions that can reach a birth and have no caller in the
        /// graph, one per line with their kind.
        #[arg(long)]
        roots: Option<PathBuf>,
        /// Fail when a native or a pointer-reached function is not covered
        /// (the gate of the link decision). Without it the check is a
        /// census: it prints the list and fails only for a vacuous rule.
        #[arg(long)]
        strict: bool,
        /// `Realm.path`: read this native as declaring nothing and being of
        /// no family (sabotage runs: the check must then fail for it).
        #[arg(long = "undeclare")]
        undeclared: Vec<String>,
        #[arg(long = "archive", required = true)]
        archives: Vec<PathBuf>,
    },
    /// Check the effect declarations of realm natives (`realm-v1.json`)
    /// against the call graph; print the census of all realm natives.
    Effects {
        #[arg(long)]
        manifest: PathBuf,
        /// Per-native census TSV.
        #[arg(long)]
        census: Option<PathBuf>,
        /// `Realm.path={ effects }`: replace one declaration (sabotage runs).
        #[arg(long = "override")]
        overrides: Vec<String>,
        #[arg(long = "archive", required = true)]
        archives: Vec<PathBuf>,
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
        Command::ThrowCensus { out, archives } => {
            perry_gc_effects::census::write(&root, &archives, &out)?
        }
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
        Command::Effects {
            manifest,
            census,
            overrides,
            archives,
        } => {
            let (g, res) = analyze(&archives, &rules, &root)?;
            let mut natives =
                perry_gc_effects::effects::natives(&std::fs::read_to_string(&manifest)?)?;
            for o in &overrides {
                let (name, text) = o
                    .split_once('=')
                    .ok_or_else(|| anyhow::anyhow!("--override wants Realm.path={{ effects }}"))?;
                let json = perry_runtime_manifest::realm_source::effects::parse_text(text)
                    .map_err(|e| anyhow::anyhow!("--override {name}: {e}"))?;
                let effects: Vec<perry_gc_effects::effects::realm_effect::RealmEffect> =
                    serde_json::from_str(&json)?;
                let mut hit = false;
                for n in natives.iter_mut().filter(|n| n.name == name) {
                    n.effects = effects.clone();
                    hit = true;
                }
                if !hit {
                    bail!("--override {name}: no such realm native");
                }
            }
            let report = perry_gc_effects::effects::check(&g, &res.effect_reach, &natives)?;
            if let Some(path) = census {
                std::fs::write(path, &report.census)?;
            }
            for line in &report.lines {
                println!("{line}");
            }
            if report.failures != 0 {
                bail!(
                    "{} effect declaration(s) contradict the call graph",
                    report.failures
                );
            }
        }
        Command::Births {
            manifest,
            birth_rules,
            census,
            roots,
            strict,
            undeclared,
            archives,
        } => {
            let (g, res) = analyze(&archives, &rules, &root)?;
            let text = std::fs::read_to_string(&manifest)?;
            let natives = perry_gc_effects::births::bodies(&text)?;
            let births = perry_gc_effects::births::declared_births(&text)?;
            let birth_text = birth_rules
                .as_ref()
                .map(std::fs::read_to_string)
                .transpose()?
                .unwrap_or_else(|| perry_gc_effects::births::RULES.into());
            let birth_rules = load_rules(&birth_text)?;
            for name in &undeclared {
                if !natives.iter().any(|n| &n.name == name) {
                    bail!("--undeclare {name}: no such realm native");
                }
            }
            let report = perry_gc_effects::births::check(
                &g,
                &res.indirect_pred,
                &birth_rules,
                &natives,
                &births,
                &undeclared,
            )?;
            if let Some(path) = census {
                std::fs::write(path, &report.census)?;
            }
            if let Some(path) = roots {
                std::fs::write(path, &report.roots)?;
            }
            for line in &report.lines {
                println!("{line}");
            }
            if strict && report.failures != 0 {
                bail!(
                    "{} realm native(s) can bear a byte cell the facts do not know of",
                    report.failures
                );
            }
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
