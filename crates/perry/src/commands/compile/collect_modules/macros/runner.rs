//! Native build-time execution uses the compiler and its existing host runtime.

use super::{value::Envelope, Invocation};
use anyhow::{bail, Context, Result};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;
use swc_ecma_ast as ast;

fn quote(value: &str) -> String {
    serde_json::to_string(value).expect("JSON string")
}

fn tail(path: &Path) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let length = file.metadata().map_or(0, |metadata| metadata.len());
    let _ = file.seek(SeekFrom::Start(length.saturating_sub(4096)));
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

pub(super) fn execute(
    importer: &Path,
    calls: &[Invocation],
    paths: &[PathBuf],
    stack: &[PathBuf],
    has_build_time_execution: &mut bool,
) -> Result<Vec<ast::Expr>> {
    let total = Instant::now();
    let temporary = tempfile::Builder::new().prefix("perry-macro-").tempdir()?;
    let root = temporary.path();
    let source = root.join("runner.ts");
    let output = root.join(if cfg!(windows) {
        "runner.exe"
    } else {
        "runner"
    });
    let result = root.join("result.json");
    let log = root.join("runner.log");
    let run_log = root.join("execution.log");
    let mut program = String::from(include_str!("runner.ts"));
    for (i, path) in paths.iter().enumerate() {
        program.push_str(&format!(
            "\nimport * as __macroImport{i} from {};\n",
            quote(&path.to_string_lossy().replace('\\', "/"))
        ));
    }
    program.push_str("\nconst __macroValues: string[] = [];\nlet __macroCurrent = '';\ntry {\n");
    for (i, call) in calls.iter().enumerate() {
        let label = format!(
            "macro import {:?} export {:?} at byte {}",
            call.specifier, call.export, call.span.lo.0
        );
        program.push_str(&format!("__macroCurrent = {};\nconst __macroArgs{i} = [{}];\n__macroValidate(__macroArgs{i}, []);\nconst __macroValue{i} = await __macroImport{i}[{}](...__macroArgs{i});\n__macroValidate(__macroValue{i}, []);\nconst __macroJson{i} = JSON.stringify(__macroValue{i});\nif (typeof __macroJson{i} !== \"string\") throw new Error(\"macro result did not serialize to JSON\");\n__macroValues.push(__macroJson{i});\n", quote(&label), call.arguments.join(","), quote(&call.export)));
    }
    program.push_str(&format!(
        concat!(
            "__macroWrite({}, {} + __macroValues.join(',') + {});\n",
            "}} catch (error) {{\n",
            "__macroWrite({}, JSON.stringify({{ok:false, error:__macroCurrent + ': ' + String(error)}}));\n",
            "process.exit(1);\n}}\n"
        ),
        quote(&result.to_string_lossy()),
        quote("{\"ok\":true,\"values\":["),
        quote("]}"),
        quote(&result.to_string_lossy()),
    ));
    fs::write(&source, program)?;
    let logs = File::create(&log)?;
    let mut compile = Command::new(std::env::current_exe()?);
    compile
        .arg("compile")
        .arg(&source)
        .arg("-o")
        .arg(&output)
        .args(["--no-cache", "--no-codegen", "--platform", "bun"])
        .arg("--cache-dir")
        .arg(root.join("objects"))
        .env_remove("PERRY_SOURCE_ROOT")
        .env_remove("PERRY_SOURCE_ROOT_TARGET")
        .env_remove("PERRY_FACTS_ONLY")
        .env_remove("PERRY_FACTS_DUMP")
        .env_remove("PERRY_LINK_PAD")
        .env_remove("PERRY_EXTRA_LINK_ARGS")
        .stdout(logs.try_clone()?)
        .stderr(logs.try_clone()?);
    for ancestor in stack
        .iter()
        .map(PathBuf::as_path)
        .chain(std::iter::once(importer))
    {
        compile.arg("--macro-import-stack").arg(ancestor);
    }
    let started = Instant::now();
    let status = compile.status().context("starting native macro compiler")?;
    let compiled = started.elapsed();
    eprintln!(
        "perry: macro runner {} compile_secs={:.3} status={status}",
        importer.display(),
        compiled.as_secs_f64()
    );
    if !status.success() {
        bail!(
            "native runner for macro imports {:?} in {} failed to compile: {}",
            calls
                .iter()
                .map(|call| (&call.specifier, &call.export))
                .collect::<Vec<_>>(),
            importer.display(),
            tail(&log)
        );
    }
    let started = Instant::now();
    let execution_log = File::create(&run_log)?;
    let status = Command::new(&output)
        .stdin(Stdio::null())
        .stdout(execution_log.try_clone()?)
        .stderr(execution_log)
        .status()
        .context("executing native macro runner")?;
    *has_build_time_execution = true;
    let executed = started.elapsed();
    std::io::copy(&mut File::open(&run_log)?, &mut std::io::stderr())
        .context("forwarding macro diagnostics")?;
    let envelope: Envelope = fs::read(&result)
        .context("macro runner did not produce a JSON result")
        .and_then(|bytes| {
            serde_json::from_slice(&bytes)
                .context("macro runner returned JSON that Perry cannot inline")
        })
        .with_context(|| {
            format!(
                "macro imports {:?} in {} (runner {status}): {}",
                calls
                    .iter()
                    .map(|call| (&call.specifier, &call.export))
                    .collect::<Vec<_>>(),
                importer.display(),
                tail(&log)
            )
        })?;
    if !envelope.ok {
        bail!(
            "{}",
            envelope
                .error
                .as_deref()
                .unwrap_or("macro runner rejected its result")
        );
    }
    if !status.success() {
        bail!(
            "macro runner in {} failed: {}",
            importer.display(),
            tail(&log)
        );
    }
    let values = envelope.values.into_iter().map(|value| value.0).collect();
    eprintln!(
        "perry: macro runner {} execute_secs={:.3} expansion_secs={:.3} status={status}",
        importer.display(),
        executed.as_secs_f64(),
        total.elapsed().as_secs_f64()
    );
    Ok(values)
}
