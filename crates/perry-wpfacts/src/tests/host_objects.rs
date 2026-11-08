//! `console` and `process.stdout` / `stderr` / `stdin` are host objects
//! perry's runtime creates: one token each, whose described members are
//! the runtime's natives until a store reaches the key (DECISIONS 255:
//! `builtins::host_written`, read by the solver's host calls and loads and
//! by the site verdict).

use super::*;

fn calls(pf: &ProgramFacts, key: &str) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.is_call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn native(pf: &ProgramFacts, key: &str) -> bool {
    let v = calls(pf, key);
    !v.is_empty() && v.iter().all(|v| *v == Verdict::NativeMember)
}

fn open(pf: &ProgramFacts, key: &str) -> bool {
    let v = calls(pf, key);
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}

const USES: &str = r#"
    console.log("ok");
    process.stdout.write("line\n");
    process.stderr.write("");
    const input = process.stdin;
    input.setEncoding("utf8");
    "#;

#[test]
fn host_members_are_the_runtimes_natives() {
    let pf = facts(USES);
    for k in ["log", "write", "setEncoding"] {
        assert!(native(&pf, k), "{k}: {:?}\n{}", calls(&pf, k), pf.dump_text());
    }
}

/// A key the runtime's layouts do not both hold is not described.
#[test]
fn an_undescribed_host_member_stays_open() {
    let pf = facts("const o = process.stdout; o.cork(); const i = process.stdin; i.setRawMode(true);");
    assert!(open(&pf, "cork"), "{:?}\n{}", calls(&pf, "cork"), pf.dump_text());
    assert!(open(&pf, "setRawMode"), "{:?}\n{}", calls(&pf, "setRawMode"), pf.dump_text());
}

/// Soundness: each of these may give the member another value.
#[test]
fn a_written_host_member_stays_open() {
    for (what, key, extra) in [
        ("a store of the member", "write", "process.stdout.write = function (s: any) { return true; } as any;"),
        ("a store into console", "log", "console.log = function (...a: any[]) {};"),
        ("a computed store", "write", "(process.stdout as any)[\"wr\" + String(Date.now() % 2)] = 1;"),
        ("a store into ⊤", "write", "import \"unseen-host\"; const t: any = (globalThis as any).someHost; t.write = 3;"),
        ("unknown code holding it", "write", "const g = new Function(\"o\", \"o.write = 1\"); g(process.stdout);"),
    ] {
        let pf = facts(&format!("console.log(\"ok\"); process.stdout.write(\"x\"); {extra}"));
        assert!(open(&pf, key), "{what}: {:?}\n{}", calls(&pf, key), pf.dump_text());
    }
}

/// A store into a host object hands the stored value to the runtime (node's
/// `console.log` calls a replaced `process.stdout.write`), so the parameter
/// of the stored function is unknown code's argument: its reads stay open.
#[test]
fn a_function_stored_on_a_host_object_is_called_by_the_runtime() {
    let pf = facts(
        "const w = function (s: any) { return s.zq; }; (process.stdout as any).write = w; console.log(\"x\");",
    );
    let v: Vec<Verdict> = pf
        .sites
        .iter()
        .filter(|s| s.key == "zq")
        .map(|s| s.verdict.clone())
        .collect();
    assert!(
        !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_))),
        "{v:?}\n{}",
        pf.dump_text()
    );
}

/// The `v` read sites on the `// SITE` line of `src`.
fn site(pf: &ProgramFacts, src: &str, key: &str) -> Vec<Verdict> {
    let m = src.find("// SITE").expect("marker");
    let start = src[..m].rfind('\n').map_or(0, |i| i + 1) as u32;
    let v: Vec<Verdict> = (pf.sites.iter())
        .filter(|s| !s.is_call && s.key == key && (start..m as u32).contains(&s.byte_offset))
        .map(|s| s.verdict.clone())
        .collect();
    assert!(!v.is_empty(), "no `{key}` site on the SITE line");
    v
}

const WRITE_RESULT: &str = "class A { v = 1; }\n\
    const a = new A();\n\
    const r: any = process.stdout.write(\"x\\n\");\n\
    if (typeof r === \"object\") r.k(a);\n";

/// Red on main (SEEDC): the stream's own `write` returns a boolean; it was
/// an unknown method whose result was ⊤, so `r.k(a)` handed `a` to unknown
/// code.
#[test]
fn a_host_stream_write_returns_a_boolean() {
    let src = format!("{WRITE_RESULT}console.log(a.v); // SITE\n");
    let pf = facts(&src);
    let v = site(&pf, &src, "v");
    assert!(v.iter().all(|v| !matches!(v, Verdict::No(_))), "{v:?}\n{}", pf.dump_text());
    assert!(native(&pf, "write"), "{:?}", calls(&pf, "write"));
}

/// Control: the stream keeps its chunk where the program can read it back
/// (and perry's write runs the chunk's `toString`): an object chunk reaches
/// unknown code, its custom `toString` included.
#[test]
fn a_host_stream_write_keeps_its_chunk() {
    let src = "class C { s = \"c\"; toString() { return this.s; } }\n\
        const c = new C();\n\
        process.stdout.write(c as any);\n\
        console.log(c.s); // SITE\n";
    let pf = facts(src);
    let v = site(&pf, src, "s");
    assert!(v.iter().all(|v| matches!(v, Verdict::No(_))), "{v:?}\n{}", pf.dump_text());
}

/// Control: `write` reads other members of its receiver (`_write`,
/// `_writableState`, `emit`): a store under any key of the stream makes the
/// call the unknown method of the host again.
#[test]
fn a_host_stream_write_after_a_store_on_the_stream_stays_open() {
    for extra in [
        "(process.stdout as any)._write = function (c: any, e: any, cb: any) { cb(); };\n",
        "import \"unseen-host\"; const t: any = (globalThis as any).someHost; t._writableState = 3;\n",
    ] {
        let src = format!("{extra}{WRITE_RESULT}console.log(a.v); // SITE\n");
        let pf = facts(&src);
        let v = site(&pf, &src, "v");
        assert!(v.iter().all(|v| matches!(v, Verdict::No(_))), "{extra}: {v:?}\n{}", pf.dump_text());
    }
}

/// node: `console.log` / `info` / `debug` call `process.stdout.write`,
/// `console.error` / `warn` call `process.stderr.write`, looked up on the
/// stream (own or inherited) at each call. A function stored there, on the
/// stream or on its prototype chain, is called by the runtime with unknown
/// arguments. Control: unstored, nothing calls it and the site is proven.
#[test]
fn a_write_stored_on_a_stream_or_its_prototype_is_called_by_console() {
    for (store, call, opened) in [
        ("void w;", "console.log(\"x\");", false),
        ("(process.stdout as any).write = w;", "console.info(\"x\");", true),
        ("(process.stderr as any).write = w;", "console.error(\"x\");", true),
        ("Object.getPrototypeOf(process.stdout).write = w;", "console.log(\"x\");", true),
        ("Object.getPrototypeOf(process.stderr).write = w;", "console.warn(\"x\");", true),
        (
            "Object.getPrototypeOf(Object.getPrototypeOf(process.stdout)).write = w;",
            "console.debug(\"x\");",
            true,
        ),
    ] {
        let src = format!(
            "class A {{ v = 1; }}\nlet seen: any = new A();\n\
             const w = function (s: any) {{ seen = s; return true; }};\n\
             {store}\n{call}\nconsole.log(seen.v); // SITE\n"
        );
        let pf = facts(&src);
        let v = site(&pf, &src, "v");
        let open = v.iter().all(|v| matches!(v, Verdict::No(_)));
        let proven = v.iter().all(|v| !matches!(v, Verdict::No(_)));
        assert!(if opened { open } else { proven }, "{store} {call}: {v:?}\n{}", pf.dump_text());
    }
}

/// A `write` stored on the stream's prototype is the stream's `write`: the
/// call is the unknown method again (its result is the stored function's).
#[test]
fn a_host_stream_write_after_a_store_on_its_prototype_stays_open() {
    let extra = "Object.getPrototypeOf(process.stdout).write = function (c: any) { return { k(x: any) { x.v = \"s\"; } }; };\n";
    let src = format!("{extra}{WRITE_RESULT}console.log(a.v); // SITE\n");
    let pf = facts(&src);
    let v = site(&pf, &src, "v");
    assert!(v.iter().all(|v| matches!(v, Verdict::No(_))), "{v:?}\n{}", pf.dump_text());
}
