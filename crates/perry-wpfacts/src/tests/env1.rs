//! ENV1 (2026-11-16). `process.env` is ONE runtime object per agent
//! (perry-runtime process/env_misc.rs `js_process_env_impl`, the
//! `CACHED_ENV` cell): an ordinary object (`js_object_alloc`, so its
//! [[Prototype]] is %Object.prototype%) whose own values are Strings: a read
//! of a present key is the variable (`process_env_get_field`: String), of an
//! absent key the chain's (OrdinaryGet 10.1.8.1 to %Object.prototype%:
//! `undefined` for any other key); a [[Set]] stores ToString(v)
//! (`process_env_set_field` -> `js_setenv`: `js_jsvalue_to_string`), never
//! `v`; delete removes the variable; for-in / Object.keys enumerate String
//! keys whose values are Strings (`refresh_process_env`). The realm declares
//! it (`process.env, kind = exotic, …, values = string`); the solver makes
//! one token for it, never a box that may hold itself.
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&list, &cfg)
}

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

fn top_origins(src: &str) -> (Vec<String>, bool) {
    let ast = Box::leak(Box::new(perry_parser::parse_typescript(src, "t.ts").unwrap()));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, true);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    (g.m.top_vals.keys().cloned().collect(), g.m.realm_held)
}

const OBJ: &str = "class N { v = 1; }\nconst n = new N();\n";

// RED (fixture gl4_fx8_env.ts): a read of `process.env` through a binding is a
// String or `undefined`, never the env object: nothing reaches readFileSync
// but a String; the realm is not held and `x.v` is proven.
#[test]
fn env1_red_env_read_through_binding() {
    let src = format!(
        "import {{ readFileSync, existsSync }} from \"node:fs\";\n{OBJ}\
         const env = process.env;\n\
         const own = env[\"GL4_NOPE_\" + (process.argv[2] ?? \"\")];\n\
         const p = own !== undefined && own !== \"\" ? own : \"/etc/hostname\";\n\
         console.log(existsSync(p) ? readFileSync(p, \"utf8\").length > 0 : false, n.v);\n\
         [n].forEach((x) => console.log(x.v));\n"
    );
    let (tops, held) = top_origins(&src);
    assert!(!held, "realm not held: {tops:?}");
    let (v, dump) = verdict(&src, "v", "x.v");
    assert!(!matches!(v, Verdict::No(_)), "x.v proven: {v:?}\n{dump}");
}

// KEEP: a program store of an object then a read: the runtime stores
// ToString(o) (o.toString runs: a program call), the read is a String.
#[test]
fn env1_keep_store_object_then_read() {
    let src = format!(
        "{OBJ}const o: any = {{ toString() {{ return \"s\" + n.v; }} }};\n\
         const env: any = process.env;\n\
         if (process.argv[2]) env.k = o;\n\
         const r: any = env.k;\n\
         console.log(typeof r, r.v);\n"
    );
    let (tops, held) = top_origins(&src);
    assert!(!held, "realm not held: {tops:?}");
    let (v, dump) = verdict(&src, "v", "r.v");
    // A String or undefined receiver: never the stored object.
    assert!(!matches!(v, Verdict::No(_)), "r.v proven: {v:?}\n{dump}");
    assert!(dump.contains(".v call=0 fixed=1 recv={primitive:string,primitive:undefined"), "r is no object\n{dump}");
    // The coercion calls the object's toString: its body runs.
    let (v, dump) = verdict(&src, "v", "n.v; }");
    assert_ne!(v, Verdict::Dead, "toString runs on the store\n{dump}");
}

// KEEP: `process.env` handed to unknown code: the escape stays visible.
#[test]
fn env1_keep_env_escapes_to_unknown_code() {
    let src = format!(
        "{OBJ}const env: any = process.env;\n\
         const g: any = globalThis;\n\
         const f: any = g[process.argv[2] ?? \"x\"];\n\
         if (typeof f === \"function\") f(env);\n\
         const r: any = env.k;\n\
         console.log(r.v);\n"
    );
    let (tops, held) = top_origins(&src);
    let (v, dump) = verdict(&src, "v", "r.v");
    assert!(held || !tops.is_empty(), "unknown code: visible {tops:?}");
    assert_ne!(format!("{v:?}"), "No(\"primitive receiver\")", "unknown code may store anything\n{dump}");
}

// KEEP: a key from argv: a String or undefined or what %Object.prototype%
// holds under that key; the realm stays free.
#[test]
fn env1_keep_key_from_argv() {
    let src = format!(
        "{OBJ}const env = process.env;\n\
         const r = env[process.argv[2] ?? \"HOME\"];\n\
         console.log(typeof r, n.v);\n\
         [n].forEach((x) => console.log(x.v));\n"
    );
    let (tops, held) = top_origins(&src);
    assert!(!held, "realm not held: {tops:?}");
    let (v, dump) = verdict(&src, "v", "x.v");
    assert!(!matches!(v, Verdict::No(_)), "x.v proven: {v:?}\n{dump}");
}

// KEEP: Object.keys( process.env ): String keys, String values.
#[test]
fn env1_keep_object_keys() {
    let src = format!(
        "{OBJ}const env = process.env;\n\
         for (const k of Object.keys(env)) {{ const s: any = env[k]; console.log(k.length, s.length); }}\n\
         [n].forEach((x) => console.log(x.v));\n"
    );
    let (tops, held) = top_origins(&src);
    assert!(!held, "realm not held: {tops:?}");
    let (v, dump) = verdict(&src, "v", "x.v");
    assert!(!matches!(v, Verdict::No(_)), "x.v proven: {v:?}\n{dump}");
}

// KEEP: delete process.env.k, then the read: undefined (or a String).
#[test]
fn env1_keep_delete() {
    let src = format!(
        "{OBJ}const env: any = process.env;\n\
         env.k = \"a\"; delete process.env.k; delete env[\"k\"];\n\
         const r: any = env.k;\n\
         console.log(r === undefined, r?.v);\n\
         [n].forEach((x) => console.log(x.v));\n"
    );
    let (tops, held) = top_origins(&src);
    assert!(!held, "realm not held: {tops:?}");
    let (v, dump) = verdict(&src, "v", "x.v");
    assert!(!matches!(v, Verdict::No(_)), "x.v proven: {v:?}\n{dump}");
}
