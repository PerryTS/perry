//! Regression test: `t(key, params)` must interpolate `{name}` placeholders.
//!
//! Closed-shape object literals are rewritten during HIR lowering into
//! `Expr::New` on a synthesized `__AnonShape_<hash>` class (constructor takes
//! the field values positionally). The i18n transform's `extract_params` only
//! recognized `Expr::Object`, so a params literal like `{ days: 5 }` yielded
//! zero lowered params and codegen's fragment plan fell back to emitting the
//! literal `{days}` text — `t("Day streak: {days}", { days: 5 })` printed
//! "Day streak: {days}" instead of "Day streak: 5" for every params object.
//! extract_params now maps the anon-shape class back to its ordered field
//! names.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn t_interpolates_params_from_object_literal() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("locales")).expect("mkdir locales");
    std::fs::create_dir_all(dir.path().join("src")).expect("mkdir src");

    std::fs::write(
        dir.path().join("perry.toml"),
        r#"
[i18n]
locales = ["en"]
default_locale = "en"
"#,
    )
    .expect("write perry.toml");

    std::fs::write(
        dir.path().join("locales/en.json"),
        r#"{
  "Day streak: {days}": "Day streak: {days}",
  "Hello, {name}! You are {age}.": "Hello, {name}! You are {age}.",
  "Für {name}!": "Für {name}!"
}"#,
    )
    .expect("write en.json");

    let entry = dir.path().join("src/main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
import { t } from "perry/i18n";

// Literal number param.
console.log(t("Day streak: {days}", { days: 5 }));
// Variable param.
const n = 6;
console.log(t("Day streak: {days}", { days: n }));
// Property-get param.
const obj = { streak: 7 };
console.log(t("Day streak: {days}", { days: obj.streak }));
// Multiple params incl. a string value.
const user = "Ada";
console.log(t("Hello, {name}! You are {age}.", { name: user, age: 36 }));
// Non-ASCII literal fragments around a placeholder must survive intact
// (the fragment parser buffers bytes and decodes once, not `b as char`).
console.log(t("Für {name}!", { name: user }));
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines,
        vec![
            "Day streak: 5",
            "Day streak: 6",
            "Day streak: 7",
            "Hello, Ada! You are 36.",
            "Für Ada!",
        ],
        "t() params must interpolate; raw {{placeholder}} output means the \
         anon-shape params regression is back.\nfull stdout:\n{}",
        stdout
    );
}

/// Multi-locale build: keys whose translations differ between locales select
/// the row at RUNTIME (perry_i18n_locale_index_for + per-locale branch in the
/// I18nString lowering). The host machine's language is not ours to pin, so
/// the assertion accepts either locale's row — what it locks in is that the
/// runtime-switch codegen compiles, runs, and interpolates params inside
/// whichever branch was taken (the pre-fix failure modes were a clang error
/// on an undeclared runtime symbol and raw `{days}` output).
#[test]
fn t_multi_locale_runtime_selection_interpolates() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("locales")).expect("mkdir locales");
    std::fs::create_dir_all(dir.path().join("src")).expect("mkdir src");

    std::fs::write(
        dir.path().join("perry.toml"),
        r#"
[i18n]
locales = ["en", "de"]
default_locale = "en"
"#,
    )
    .expect("write perry.toml");
    std::fs::write(
        dir.path().join("locales/en.json"),
        r#"{ "Day streak: {days}": "Day streak: {days}" }"#,
    )
    .expect("write en.json");
    std::fs::write(
        dir.path().join("locales/de.json"),
        r#"{ "Day streak: {days}": "Serie: {days} Tage" }"#,
    )
    .expect("write de.json");

    let entry = dir.path().join("src/main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
import { t } from "perry/i18n";
console.log(t("Day streak: {days}", { days: 9 }));
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    let line = stdout.lines().next().unwrap_or("");
    assert!(
        line == "Day streak: 9" || line == "Serie: 9 Tage",
        "expected the en or de row with {{days}} interpolated, got: {:?}",
        line
    );
}
