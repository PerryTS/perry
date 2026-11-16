//! Rule M3 for a key parameter that is also passed: three.js `Cache`
//! (three.core.js ~44340) `add(key, file) { if (isBlobURL(key)) return;
//! this.files[key] = file; }` and `get(key) { if (isBlobURL(key)) return;
//! return this.files[key]; }`, called with `` `file:${url}` `` (FileLoader),
//! `` `image:${url}` `` (ImageLoader) and `` `image-bitmap:${url}` ``
//! (ImageBitmapLoader). Passing a String as an argument copies the value
//! (ECMA-262 10.2.1, 7.1.19 ToPropertyKey of a String is the String), so a
//! run of one key value passes that value and keys by it alone: each run is
//! still one key. Strings with incompatible known prefixes are never equal
//! (7.2.13): the `image-bitmap:` read never reads what the `file:` store
//! wrote (the FileLoader ArrayBuffer and JSON responses reached the
//! GLTFLoader texture through it).
//! Controls: a key from `process.argv[2]`, a `JSON.parse(process.argv[2])`
//! value used as a key (may be `constructor`), unknown code
//! (`new Function(process.argv[2])`) given the cache. Receiver use: the key
//! read as a receiver (`key.length`, `key.startsWith(..)`, `key.slice(..)`
//! in isBlob) binds the same value and changes nothing of it (7.3.2 GetV),
//! so each run is still one key.
use super::*;

/// The sorted class names (or other tokens) the `.p` reads see.
fn recv(pf: &ProgramFacts) -> Vec<String> {
    let mut r: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == "p" && !s.is_call)
        .flat_map(|s| s.receivers.iter())
        .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
        .collect();
    r.sort();
    r.dedup();
    r
}

fn prog(tail: &str) -> String {
    format!(
        "class A {{ p = 1; }}
         class B {{ p = 2; }}
         function isBlob(key: any): any {{
           try {{ const s = key.slice(key.indexOf(':') + 1); return s === 'blob'; }} catch (e) {{ return false; }}
         }}
         const Cache: any = {{
           files: {{}},
           add: function (key: any, file: any) {{ if (isBlob(key)) return; this.files[key] = file; }},
           get: function (key: any) {{ if (isBlob(key)) return; return this.files[key]; }},
         }};
         const u = process.argv[3];
         Cache.add(`file:${{u}}`, new B());
         Cache.add(`image:${{u}}`, new A());
         {tail}"
    )
}

fn reads(tail: &str) -> Vec<String> {
    recv(&facts(&prog(tail)))
}

#[test]
fn passed_key_runs_per_prefix() {
    let v = reads("const x: any = Cache.get(`image:${u}`); console.log(x.p);");
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn passed_key_runs_per_concatenated_prefix() {
    let v = reads("Cache.add('b:' + u, new B()); const x: any = Cache.get('image:' + u); console.log(x.p);");
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn control_argv_key_reads_every_entry() {
    let v = reads("const x: any = Cache.get(process.argv[2]); console.log(x.p);");
    assert!(v.contains(&"A".into()) && v.contains(&"B".into()) && v.len() > 2, "{v:?}");
}

#[test]
fn control_json_key_may_name_a_member() {
    let v = reads("const k: any = JSON.parse(process.argv[2]); const x: any = Cache.get(k.uri); console.log(x.p);");
    assert!(v.contains(&"B".into()) && v.len() > 2, "control: a JSON string may be constructor: {v:?}");
}

#[test]
fn control_unknown_code_writes_the_cache() {
    let v = reads(
        "const f: any = new Function(process.argv[2]); f(Cache); const x: any = Cache.get(`image:${u}`); console.log(x.p);",
    );
    assert_ne!(v, vec!["A".to_string()], "control: unknown code may store anything: {v:?}");
}

#[test]
fn key_used_only_as_key_runs_per_prefix() {
    let v = recv(&facts(&prog("const x: any = Cache.get(`image:${u}`); console.log(x.p);").replace("if (isBlob(key)) return; ", "")));
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn receiver_use_keeps_per_key_runs() {
    // The key read as a receiver in the body itself (no call passing it on):
    // `key.length` and `key.startsWith('x')` read through the String and
    // change nothing of it, so each run still holds one key value.
    let v = recv(&facts(
        &prog("const x: any = Cache.get(`image:${u}`); console.log(x.p);")
            .replace("if (isBlob(key)) return; ", "if (key.length === 0 || key.startsWith('x')) return; "),
    ));
    assert_eq!(v, vec!["A"], "{v:?}");
}
