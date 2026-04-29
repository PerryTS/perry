//! Styling-matrix generator + drift checker (Phase A of issue #185).
//!
//! Two modes:
//!   `--gen`   write `docs/src/ui/styling-matrix.md` from `MATRIX`
//!   `--check` scan each `crates/perry-ui-*/src/lib.rs` and verify that
//!             every Wired cell has a matching `pub extern "C" fn perry_ui_*`
//!             symbol, and that no platform's `lib.rs` exports a symbol the
//!             matrix doesn't know about. Exits 1 on drift.
//!
//! Both modes are run by `scripts/run_ui_styling_matrix.sh` in CI.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use perry_ui::styling_matrix::{drift, Platform, Status, MATRIX};

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at crates/perry-ui; go up two levels.
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn cmd_check(root: &PathBuf) -> ExitCode {
    let drifts = drift::check_all(root);
    let mut any_drift = false;
    let mut report = String::new();

    for d in &drifts {
        if d.is_clean() {
            continue;
        }
        any_drift = true;
        let plat_name = d.platform.map(|p| p.name()).unwrap_or("?");
        report.push_str(&format!("\n=== {} drift ===\n", plat_name));
        if !d.wired_but_missing.is_empty() {
            report.push_str("  matrix claims Wired/Stub but symbol absent from lib.rs:\n");
            for s in &d.wired_but_missing {
                report.push_str(&format!("    - {}\n", s));
            }
        }
        if !d.present_but_marked_missing.is_empty() {
            report.push_str("  symbol present in lib.rs but matrix says Missing/NotApplicable:\n");
            for s in &d.present_but_marked_missing {
                report.push_str(&format!("    - {}\n", s));
            }
        }
    }

    if any_drift {
        eprintln!("STYLING MATRIX DRIFT DETECTED");
        eprintln!("{}", report);
        eprintln!("Update crates/perry-ui/src/styling_matrix.rs to match reality, or");
        eprintln!("update the affected platform's lib.rs to match the matrix.");
        ExitCode::from(1)
    } else {
        println!(
            "styling matrix: OK ({} rows × {} platforms checked)",
            MATRIX.len(),
            drifts.len()
        );
        let _ = Status::Wired; // keep import live for compile
        ExitCode::SUCCESS
    }
}

fn status_cell(s: Status) -> &'static str {
    match s {
        Status::Wired => "✓",
        Status::Stub => "~",
        Status::Missing => "✗",
        Status::NotApplicable => "—",
    }
}

fn cmd_gen(root: &PathBuf) -> ExitCode {
    let mut md = String::new();
    md.push_str("# perry/ui styling matrix\n\n");
    md.push_str("Auto-generated from `crates/perry-ui/src/styling_matrix.rs` by ");
    md.push_str("`scripts/run_ui_styling_matrix.sh`. Do not edit by hand — ");
    md.push_str("CI fails if this file drifts from the source-of-truth.\n\n");
    md.push_str("Legend: `✓` Wired (real native impl), `~` Stub (symbol exists, no-op), ");
    md.push_str("`✗` Missing (FFI symbol not exported), `—` Not applicable to this platform.\n\n");

    // Group rows by widget.
    let mut by_widget: BTreeMap<&str, Vec<&_>> = BTreeMap::new();
    for row in MATRIX {
        by_widget.entry(row.widget).or_default().push(row);
    }

    // Stable section order: generic ("*") first, then alphabetical.
    let mut widgets: Vec<&str> = by_widget.keys().copied().collect();
    widgets.sort_by(|a, b| match (*a, *b) {
        ("*", "*") => std::cmp::Ordering::Equal,
        ("*", _) => std::cmp::Ordering::Less,
        (_, "*") => std::cmp::Ordering::Greater,
        _ => a.cmp(b),
    });

    for widget in widgets {
        let title = if widget == "*" {
            "Generic widget setters (apply to any widget)".to_string()
        } else {
            format!("`{}` widget", widget)
        };
        md.push_str(&format!("## {}\n\n", title));

        // Header row.
        md.push_str("| Prop | FFI symbol |");
        for p in Platform::ALL {
            md.push_str(&format!(" {} |", p.name()));
        }
        md.push('\n');
        md.push_str("|---|---|");
        for _ in Platform::ALL {
            md.push_str("---|");
        }
        md.push('\n');

        for row in by_widget.get(widget).unwrap() {
            md.push_str(&format!("| `{}` | `{}` |", row.prop, row.ffi));
            for p in Platform::ALL {
                md.push_str(&format!(" {} |", status_cell(row.status(*p))));
            }
            md.push('\n');
        }
        md.push('\n');
    }

    // Summary stats.
    md.push_str("## Summary\n\n");
    md.push_str("| Platform | Wired | Stub | Missing | Not applicable |\n");
    md.push_str("|---|---|---|---|---|\n");
    for plat in Platform::ALL {
        let mut counts = [0usize; 4];
        for row in MATRIX {
            let i = match row.status(*plat) {
                Status::Wired => 0,
                Status::Stub => 1,
                Status::Missing => 2,
                Status::NotApplicable => 3,
            };
            counts[i] += 1;
        }
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            plat.name(),
            counts[0],
            counts[1],
            counts[2],
            counts[3]
        ));
    }
    md.push('\n');

    let out = root.join("docs/src/ui/styling-matrix.md");
    if let Some(parent) = out.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("create dir {}: {}", parent.display(), e);
            return ExitCode::from(2);
        }
    }
    if let Err(e) = fs::write(&out, &md) {
        eprintln!("write {}: {}", out.display(), e);
        return ExitCode::from(2);
    }
    println!("wrote {}", out.display());
    ExitCode::SUCCESS
}

fn cmd_diff(root: &PathBuf) -> ExitCode {
    // Generate fresh content and compare to disk; fail on diff.
    let out = root.join("docs/src/ui/styling-matrix.md");
    let on_disk = fs::read_to_string(&out).unwrap_or_default();

    // Re-run gen into a temp string by capturing what cmd_gen would write.
    // Simpler: write to disk, then compare. But that mutates the tree on
    // CI which is fine — `git diff --exit-code` after the script's gen
    // step is the actual drift signal in CI. So `--diff` is just a local
    // helper that runs gen then prints whether content changed.
    if cmd_gen(root) != ExitCode::SUCCESS {
        return ExitCode::from(2);
    }
    let after = fs::read_to_string(&out).unwrap_or_default();
    if on_disk == after {
        println!("styling-matrix.md: no changes");
        ExitCode::SUCCESS
    } else {
        eprintln!("styling-matrix.md regenerated with changes; commit the diff");
        ExitCode::from(1)
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let root = workspace_root();
    match args.first().map(|s| s.as_str()) {
        Some("--check") => cmd_check(&root),
        Some("--gen") => cmd_gen(&root),
        Some("--diff") => cmd_diff(&root),
        _ => {
            eprintln!("usage: styling-matrix [--check | --gen | --diff]");
            eprintln!("  --check  verify MATRIX matches lib.rs exports across all platforms");
            eprintln!("  --gen    write docs/src/ui/styling-matrix.md");
            eprintln!(
                "  --diff   regenerate and report whether docs/src/ui/styling-matrix.md changed"
            );
            ExitCode::from(2)
        }
    }
}
