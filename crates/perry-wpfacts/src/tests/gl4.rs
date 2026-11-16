//! GLOB4: the URL object as perry's runtime makes it. Its numbered fields
//! are private storage; `href` & co. are accessors on URL.prototype whose
//! getters return the String in the slot (`url/prototype.rs` url_getter,
//! `url/parse.rs` create_url_object: every slot is set with a String) and
//! `searchParams` the URLSearchParams object the constructor made.
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    compute(&[("t.ts".to_string(), &hir)], &cfg)
}

/// The verdict of the first site `.key` after `marker`, and the dump.
fn verdict(src: &str, key: &str, marker: &str) -> (Verdict, String) {
    let pf = facts_of(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()));
    (s.verdict.clone(), pf.dump_text())
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

const URL_HEAD: &str = "class N { v = 1; }
        const n = new N();
        const u = new URL(\"./audio/\", \"file:///x/\" + (process.argv[2] ?? \"a\") + \"/\");\n";

/// RED: `href` reads a String: the method call on it is a String method,
/// and the program's objects do not reach unknown code.
#[test]
fn gl4_url_href_is_string() {
    let src = format!(
        "{URL_HEAD}const base = u.href;
        const b = /*A*/ base.endsWith(\"/\") ? base : base + \"/\";
        [n].forEach((x) => console.log(x.v, b.length > 0));
        console.log(/*B*/ n.v);"
    );
    let (v, dump) = verdict(&src, "endsWith", "/*A*/");
    assert!(proven(&v), "base.endsWith proven, got {v:?}\n{dump}");
    let (v, dump) = verdict(&src, "v", "/*B*/");
    assert!(proven(&v), "n.v proven, got {v:?}\n{dump}");
}

/// RED: every String member of a URL (WHATWG URL 6.1) reads a String.
#[test]
fn gl4_url_string_members() {
    for m in [
        "href", "origin", "protocol", "username", "password", "host", "hostname", "port",
        "pathname", "search", "hash",
    ] {
        let src = format!("{URL_HEAD}console.log(/*A*/ u.{m}.endsWith(\"/\"));");
        let (v, dump) = verdict(&src, "endsWith", "/*A*/");
        assert!(proven(&v), "u.{m}.endsWith proven, got {v:?}\n{dump}");
    }
}

/// RED: `searchParams` is the URL's URLSearchParams object: its `get` is
/// the builtin method; `toString` / `toJSON` of a URL return a String.
#[test]
fn gl4_url_search_params_and_serializers() {
    let src = format!(
        "{URL_HEAD}const sp = u.searchParams;
        console.log(/*A*/ sp.get(\"a\"));
        console.log(/*B*/ u.toString().endsWith(\"/\"), /*C*/ u.toJSON().endsWith(\"/\"));"
    );
    // The receiver is exactly the URLSearchParams object (a builtin
    // receiver: no fact unit, as for any builtin object), never unknown.
    let (v, dump) = verdict(&src, "get", "/*A*/");
    let d = format!("{v:?}");
    assert!(
        d.contains("builtin URLSearchParams") && !d.contains("TOP"),
        "sp is the URLSearchParams: {d}\n{dump}"
    );
    let (v, dump) = verdict(&src, "endsWith", "/*B*/");
    assert!(proven(&v), "toString().endsWith proven, got {v:?}\n{dump}");
    let (v, dump) = verdict(&src, "endsWith", "/*C*/");
    assert!(proven(&v), "toJSON().endsWith proven, got {v:?}\n{dump}");
}

/// KEEP: `URL.prototype.href` replaced by a program getter (chosen from
/// argv) that returns a program object: a read of `href` sees that object;
/// the read of `.v` on it is never claimed as a String member nor Dead.
#[test]
fn gl4_keep_href_getter_replaced() {
    let src = format!(
        "{URL_HEAD}if (process.argv[2] === \"x\") Object.defineProperty(URL.prototype, \"href\", {{ get() {{ return n; }} }});
        const h: any = u.href;
        console.log(/*A*/ h.v);"
    );
    let (v, dump) = verdict(&src, "v", "/*A*/");
    let d = format!("{v:?}");
    assert!(
        !d.contains("Dead") && !d.contains("NativeMember"),
        "h.v must see N: {d}\n{dump}"
    );
}

/// KEEP: a subclass of URL: its own member is the program's; `href` still
/// the URL getter's String.
#[test]
fn gl4_keep_url_subclass() {
    let src = "class N { v = 1; }
        class U extends URL { tag = new N(); }
        const u = new U(\"file:///x/\" + (process.argv[2] ?? \"a\"));
        console.log(/*A*/ u.tag.v, u.href.length);";
    let (v, dump) = verdict(src, "v", "/*A*/");
    let d = format!("{v:?}");
    assert!(!d.contains("Dead"), "u.tag.v live: {d}\n{dump}");
}

/// KEEP: an argument object with a program `toString` (chosen from argv):
/// the constructor's ToString runs it (WebIDL USVString): the store it
/// makes is seen by a later read.
#[test]
fn gl4_keep_argument_to_string_runs() {
    let src = "class N { v = 1; w = 0; }
        const n = new N();
        const arg = { toString() { n.w = Number(process.argv[2]); return \"file:///x/\"; } };
        const u = new URL(process.argv[2] === \"x\" ? arg : \"file:///y/\");
        console.log(u.href, /*A*/ n.w);";
    let (v, dump) = verdict(src, "w", "/*A*/");
    let d = format!("{v:?}");
    assert!(!d.contains("Dead"), "n.w live: {d}\n{dump}");
}

/// KEEP: `u.href = v` then a read: the setter parses `v` (ToString) and the
/// getter reads a String; a stored program object never reads back as a
/// URL member that is not there.
#[test]
fn gl4_keep_href_set_then_read() {
    let src = format!(
        "{URL_HEAD}const arg = {{ toString() {{ return \"file:///z/\" + n.v; }} }};
        (u as any).href = process.argv[2] === \"x\" ? arg : \"file:///q/\";
        const h: any = u.href;
        console.log(/*A*/ h.endsWith(\"/\"));"
    );
    let (v, dump) = verdict(&src, "endsWith", "/*A*/");
    let d = format!("{v:?}");
    assert!(!d.contains("Dead"), "h.endsWith live: {d}\n{dump}");
}
