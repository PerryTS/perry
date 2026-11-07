//! A node module member whose effects the runtime declares at the body its
//! dispatch arm calls (an effects-only `realm_decl!` above
//! `js_fs_exists_sync`) runs that declaration: `fs.existsSync(p)` reads
//! what node's getValidatedPath reads off `p` (`path(arg(0))`) and returns
//! a boolean. It calls none of `p`'s methods and hands `p` to no one.
use super::*;

const FORMS: [(&str, &str); 3] = [
    ("import * as fs from \"fs\";", "fs.existsSync"),
    ("import { existsSync } from \"node:fs\";", "existsSync"),
    ("const fs = require(\"fs\");", "fs.existsSync"),
];

/// The argument's methods are not called back by unknown code: `m`'s
/// parameter holds only what the program passes.
#[test]
fn exists_sync_does_not_call_its_argument_back() {
    for (import, call) in FORMS {
        let pf = facts(&format!(
            r#"
            {import}
            class Q {{ y = 1; }}
            const o: any = {{ m(a: any) {{ return a.y; }} }};
            console.log({call}(o));
            console.log(o.m(new Q()));
        "#
        ));
        let y = site(&pf, "y");
        assert!(
            !y.top && y.receivers.iter().all(|r| r.ends_with(":Q")),
            "{import}\n{}",
            pf.dump_text()
        );
    }
}

/// The result is a primitive (the declared boolean) in every import form.
#[test]
fn exists_sync_result_is_a_primitive() {
    for (import, call) in FORMS {
        let pf = facts(&format!(
            r#"
            {import}
            function f(v: any) {{ return v.q; }}
            console.log(f({call}("/tmp")));
        "#
        ));
        let q = site(&pf, "q");
        assert!(
            !q.top && q.receivers.is_empty(),
            "{import}\n{}",
            pf.dump_text()
        );
    }
}

/// Named control: the getters node's path check runs stay modelled, one
/// step deep (`href`) and two (`pathname.length`), and so does the
/// ToString of the pathname: what they store is seen.
#[test]
fn exists_sync_runs_the_path_getters() {
    for arg in [
        "{ get href() { p.x = \"s\"; return undefined; } }",
        "{ href: \"file:///t\", protocol: \"file:\", hostname: \"\", pathname: { get length() { p.x = \"s\"; return 0; } } }",
        "{ href: \"file:///t\", protocol: \"file:\", hostname: \"\", pathname: { toString() { p.x = \"s\"; return \"/t\"; } } }",
    ] {
        let pf = facts(&format!(
            r#"
            import * as fs from "fs";
            class P {{ x: any; constructor(x: number) {{ this.x = x; }} }}
            const p = new P(1);
            const o: any = {arg};
            console.log(fs.existsSync(o));
            console.log(p.x);
        "#
        ));
        let x = field(tmpl(&pf, "P"), "x");
        assert!(!x.readonly, "{arg}\n{}", pf.dump_text());
    }
}

/// Named control: the error message's `util.inspect(v, { depth: -1 })`
/// (`ShallowInspect`) reads an Error's `stack` and `message`, getters
/// included: what they store is seen. (Its `[util.inspect.custom]` and
/// `@@toStringTag` are computed members, which every named read sees.)
#[test]
fn exists_sync_runs_the_inspect_reads() {
    for arg in [
        "{ get stack() { p.x = \"s\"; return \"e\"; } }",
        "{ get message() { p.x = \"s\"; return \"e\"; } }",
    ] {
        let pf = facts(&format!(
            r#"
            import * as fs from "fs";
            class P {{ x: any; constructor(x: number) {{ this.x = x; }} }}
            const p = new P(1);
            const o: any = {arg};
            console.log(fs.existsSync(o));
            console.log(p.x);
        "#
        ));
        let x = field(tmpl(&pf, "P"), "x");
        assert!(!x.readonly, "{arg}\n{}", pf.dump_text());
    }
}

/// Node reads only the keys of its path check: a getter under any other
/// key never runs, so what it would store is not seen (main: the argument
/// was read under any key).
#[test]
fn exists_sync_reads_only_what_node_reads() {
    let pf = facts(
        r#"
        import * as fs from "fs";
        class P { x: any; constructor(x: number) { this.x = x; } }
        const p = new P(1);
        const o: any = { get other() { p.x = "s"; return 1; } };
        console.log(fs.existsSync(o));
        console.log(p.x);
    "#,
    );
    let x = field(tmpl(&pf, "P"), "x");
    assert!(x.readonly, "{}", pf.dump_text());
}
