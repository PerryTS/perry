//! An fs path operand (`fs.mkdirSync(p)`, `unlinkSync`, `rmSync`,
//! `existsSync`) is read as node's getValidatedPath reads it: a string or a
//! Buffer as is, a URL by its named keys (`href`, `protocol`, `pathname`,
//! ...), anything else is ERR_INVALID_ARG_TYPE. No ToPrimitive and no call
//! of a function found under some key (tssrc sys.ts:1550
//! `_fs.mkdirSync(directoryName)`, where a function value reaches the path
//! through the string over-merge): such a function is not called back and
//! what it would leak does not escape.
use super::*;

/// The fs path calls; `X` is the operand.
const CALLS: &[(&str, &str)] = &[
    ("mkdir", "fs.mkdirSync(X)"),
    ("unlink", "fs.unlinkSync(X)"),
    (
        "rm_recursive",
        "fs.rmSync(X, { recursive: true, force: true })",
    ),
    ("exists", "fs.existsSync(X)"),
];

const HEAD: &str = r#"
import * as fs from "fs";
class Payload { n = 1; }
const payload = new Payload();
"#;

fn verdict_ok(src: &str, proven: bool) -> Result<(), String> {
    let marker = "// WITNESS";
    let at = src.find(marker).expect("named marker");
    let start = src[..at].rfind('\n').map_or(0, |n| n + 1) as u32;
    let pf = facts(src);
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == "n" && (start..at as u32).contains(&s.byte_offset))
        .collect();
    if sites.is_empty() {
        return Err(format!("missing witness\n{}", pf.dump_text()));
    }
    for s in sites {
        if !matches!(s.verdict, Verdict::No(_)) != proven {
            return Err(format!("witness: {:?}\n{}", s.verdict, pf.dump_text()));
        }
    }
    Ok(())
}

fn matrix(make: impl Fn(&str) -> String, proven: bool) -> Vec<String> {
    let mut failures = Vec::new();
    for (name, call) in CALLS {
        if let Err(error) = verdict_ok(&make(call), proven) {
            failures.push(format!("{name}: {}", error.lines().next().unwrap()));
        }
    }
    failures
}

/// A function value as the path: not called back, `payload` stays proven.
#[test]
fn fs_path_function_operand_is_not_called_back() {
    let f = matrix(
        |call| {
            format!(
                "{HEAD}\nfunction createDirectory(name: any) {{ name.saved = payload; return \"d\"; }}\ncreateDirectory({{}});\ntry {{ {}; }} catch (e) {{}}\nconsole.log(payload.n); // WITNESS\n",
                call.replace('X', "(createDirectory as any)")
            )
        },
        true,
    );
    assert!(f.is_empty(), "fs path operand called back: {f:?}");
}

/// An object as the path: a method no path check reads never runs.
#[test]
fn fs_path_object_operand_method_never_runs() {
    let f = matrix(
        |call| {
            format!(
                "{HEAD}\nconst o = {{ s: \"x\", f(a: any) {{ a.saved = payload; return 1; }} }};\no.f({{}});\ntry {{ {}; }} catch (e) {{}}\nconsole.log(payload.n); // WITNESS\n",
                call.replace('X', "(o as any)")
            )
        },
        true,
    );
    assert!(f.is_empty(), "fs path operand method ran: {f:?}");
}

/// Named control: a URL-like path's `href` getter runs (node's isURL reads
/// it) and hands `payload` to unknown code: the witness is not proven.
#[test]
fn fs_path_url_getter_runs() {
    let f = matrix(
        |call| {
            format!(
                "{HEAD}\nconst u = {{ get href() {{ new Function(process.argv[2])(payload); return \"file:///tmp/x\"; }}, protocol: \"file:\", pathname: \"/tmp/x\" }};\ntry {{ {}; }} catch (e) {{}}\nconsole.log(payload.n); // WITNESS\n",
                call.replace('X', "(u as any)")
            )
        },
        false,
    );
    assert!(f.is_empty(), "URL getter not run: {f:?}");
}

/// Control: a real string path keeps the program's facts.
#[test]
fn fs_path_string_operand_keeps_facts() {
    let f = matrix(
        |call| {
            format!(
                "{HEAD}\ntry {{ {}; }} catch (e) {{}}\nconsole.log(payload.n); // WITNESS\n",
                call.replace('X', "\"/tmp/fspath-x\"")
            )
        },
        true,
    );
    assert!(f.is_empty(), "string path lost facts: {f:?}");
}
