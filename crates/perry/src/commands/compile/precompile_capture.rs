//! #1681 (Phase 3 of #1677) — self-hosted build-time evaluation of
//! `precompile(...)` codegen.
//!
//! Perry runs the codegen itself, with **no node and no embedded JS engine**:
//! it compiles the entry program to a native binary in *capture mode* (each
//! `precompile(EXPR)` site lowered to a `console.log` that prints `EXPR`'s
//! build-time value), runs that binary — Perry executing its own compiled
//! output — and parses the emitted generated sources back. The main compile
//! then substitutes the natively-compiled generated function for each site
//! (see `perry-hir`'s `try_precompile`).
//!
//! The capture binary is invoked via `current_exe`, so the "build-time
//! evaluator" is just Perry compiling-and-running code — the same way the
//! driver shells out to `cc` to link. The shipped binary contains no engine.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, Result};

use super::{CompilationContext, CompileArgs};
use crate::OutputFormat;

const CAPTURE_ENV: &str = "PERRY_PRECOMPILE_CAPTURE";
const MARKER: &str = "\u{1}PERRY_PRECOMPILE\u{1}";

/// Driver entry, run just before module collection. Records its result on
/// `ctx` (`precompile_capture` / `precompile_results`); `collect_modules`
/// re-installs those onto the lowering thread before each module (the
/// thread-locals don't survive a hop to a rayon worker, so per-lower
/// re-installation is required — same pattern as the `#665`/`#503` config).
///
/// - In the capture subprocess (`PERRY_PRECOMPILE_CAPTURE` set): mark this
///   compile as the capture stage and return (no recursion).
/// - Otherwise: if the entry source uses `precompile(`, run the build-time
///   capture stage and stash its results for the main compile.
pub(super) fn prepare_precompile(
    args: &CompileArgs,
    ctx: &mut CompilationContext,
    format: OutputFormat,
) -> Result<()> {
    if std::env::var(CAPTURE_ENV).is_ok() {
        ctx.precompile_capture = true;
        return Ok(());
    }
    // Cheap gate: only pay for the capture stage when the entry mentions
    // `precompile(` at all.
    let entry_src = std::fs::read_to_string(&args.input).unwrap_or_default();
    if !entry_src.contains("precompile(") {
        return Ok(());
    }
    let results = run_capture(args, format)?;
    if matches!(format, OutputFormat::Text) {
        println!(
            "  Precompile: evaluated {} build-time site(s)",
            results.len()
        );
    }
    ctx.precompile_results = results;
    Ok(())
}

/// Compile the entry in capture mode via Perry's own binary, run the produced
/// executable, and parse the emitted `(file, span) -> generated source` map.
fn run_capture(args: &CompileArgs, format: OutputFormat) -> Result<HashMap<(String, u32), String>> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow!("precompile: cannot locate the Perry binary: {e}"))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_bin =
        std::env::temp_dir().join(format!("perry_precompile_{}_{}", std::process::id(), stamp));

    // Stage 1a — compile the entry in capture mode.
    let compile = Command::new(&exe)
        .arg("compile")
        .arg(&args.input)
        .arg("-o")
        .arg(&tmp_bin)
        .env(CAPTURE_ENV, "1")
        // Keep the build-time helper fast and faithful to source.
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .output()
        .map_err(|e| anyhow!("precompile: failed to spawn the capture compile: {e}"))?;
    if !compile.status.success() {
        let _ = cleanup(&tmp_bin);
        return Err(anyhow!(
            "precompile: build-time capture compile failed ({})\n--- stderr ---\n{}",
            compile.status,
            String::from_utf8_lossy(&compile.stderr).trim_end(),
        ));
    }

    // Stage 1b — run the capture binary. Tolerate a non-zero exit: the
    // `precompile` sites emit their source during module init, before any
    // downstream use of the (now-undefined) result might fault; the markers
    // are already on stdout.
    let run = Command::new(&tmp_bin)
        .output()
        .map_err(|e| anyhow!("precompile: failed to run the capture binary: {e}"))?;
    let stdout = String::from_utf8_lossy(&run.stdout);
    let map = parse_markers(&stdout);
    if map.is_empty() && matches!(format, OutputFormat::Text) {
        eprintln!(
            "  Precompile: capture run emitted no results (exit {}). stderr:\n{}",
            run.status,
            String::from_utf8_lossy(&run.stderr).trim_end(),
        );
    }
    let _ = cleanup(&tmp_bin);
    Ok(map)
}

/// Parse `\x01PERRY_PRECOMPILE\x01<file>\x01<span_lo>\x01<json-encoded src>`
/// lines from the capture binary's stdout.
fn parse_markers(stdout: &str) -> HashMap<(String, u32), String> {
    let mut map = HashMap::new();
    for line in stdout.lines() {
        let Some(pos) = line.find(MARKER) else {
            continue;
        };
        // Everything from the marker on; split on the SOH delimiter. The
        // JSON-encoded source can't contain a raw SOH (it'd be ``), so
        // the field count is stable.
        let parts: Vec<&str> = line[pos..].split('\u{1}').collect();
        // parts == ["", "PERRY_PRECOMPILE", <file>, <lo>, <json src>]
        if parts.len() < 5 || parts[1] != "PERRY_PRECOMPILE" {
            continue;
        }
        let file = parts[2].to_string();
        let Ok(lo) = parts[3].parse::<u32>() else {
            continue;
        };
        if let Ok(src) = serde_json::from_str::<String>(parts[4]) {
            map.insert((file, lo), src);
        }
    }
    map
}

fn cleanup(path: &PathBuf) -> std::io::Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_marker_line() {
        let out = format!(
            "some normal output\n{MARKER}/app/main.ts\u{1}42\u{1}{}\nmore output\n",
            "\"(a) => a + 3\""
        );
        let map = parse_markers(&out);
        assert_eq!(
            map.get(&("/app/main.ts".to_string(), 42))
                .map(String::as_str),
            Some("(a) => a + 3")
        );
    }

    #[test]
    fn ignores_non_marker_output() {
        let map = parse_markers("hello\nworld\n");
        assert!(map.is_empty());
    }

    #[test]
    fn handles_multiline_source_via_json_escaping() {
        // JSON-encoded source with an escaped newline stays on one line.
        let out = format!(
            "{MARKER}/x.ts\u{1}7\u{1}{}\n",
            "\"function (a) {\\n  return a\\n}\""
        );
        let map = parse_markers(&out);
        assert_eq!(
            map.get(&("/x.ts".to_string(), 7)).map(String::as_str),
            Some("function (a) {\n  return a\n}")
        );
    }
}
