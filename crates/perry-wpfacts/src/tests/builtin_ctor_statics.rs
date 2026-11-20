//! A named read on a built-in constructor whose statics the realm declares
//! (`Date`, `ArrayBuffer`, `SharedArrayBuffer`, `DataView`, `%TypedArray%`
//! and each concrete typed array) finds that constructor's own statics, then
//! its [[Prototype]]'s; a key on neither is absent (`undefined`), unless the
//! program may have written it. three.js: `/re/.source.replace(..)` reads
//! `replace` on every builtin value the regexp's `source` may be, the
//! typed-array constructors among them; read as an unknown static, it
//! minted an unknown builtin function whose call escaped every builtin value
//! and so the global object.
use super::*;

fn facts(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let mut pb = perry_hir::patched_builtins::PatchedBuiltins::new();
    perry_hir::scan_patched_builtins(&module, &mut pb);
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    perry_hir::patched_builtins::set_patched_builtins(std::sync::Arc::new(pb));
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    perry_hir::patched_builtins::clear_patched_builtins();
    crate::compute(&[(path, &hir)], &cfg)
}

/// Is the site `key` (a call or a read) at the last `/*A*/` proven?
fn proven(src: &str, key: &str, call: bool) -> (bool, String) {
    let pf = facts(src);
    let at = src.rfind("/*A*/").expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after /*A*/ in\n{}", pf.dump_text()));
    (
        !matches!(s.verdict, Verdict::No(_)),
        format!("{:?}", s.verdict),
    )
}

/// `Int16Array.replace` is undefined (node): calling it throws, nothing
/// escapes, and `o`, only given to `console.log`, keeps its fields.
#[test]
fn absent_static_of_a_typed_array_constructor_is_undefined() {
    let src = "class K { m(): number { return 1; } }
        const o: any = { k: new K() };
        const T: any = { Int16Array: Int16Array };
        const s = /a(b)/.source.replace(\"b\", \"c\");
        console.log(o, s.length, T.Int16Array.BYTES_PER_ELEMENT);
        console.log(/*A*/ o.k.m());";
    let (ok, v) = proven(src, "m", true);
    assert!(ok, "o.k.m(): {v}");
}

/// three.js' shape: two typed-array constructors held, one constructed,
/// and `.source.replace` on a regexp. The global object does not escape:
/// a value the program stores on it keeps its methods.
#[test]
fn regexp_source_replace_beside_typed_array_constructors() {
    let src = "class K { m(): number { return 1; } }
        (globalThis as any).g = new K();
        const T: any = { Int16Array: Int16Array, Float32Array: Float32Array };
        const a = new T.Int16Array(2);
        const s = /((?:WC+[\\/:])*)/.source.replace(\"WC\", \"x\");
        console.log(s.length, a.length);
        console.log(/*A*/ (globalThis as any).g.m());";
    let (ok, v) = proven(src, "m", true);
    assert!(ok, "globalThis.g.m(): {v}");
}

/// Named control: a computed-key store onto the constructor may define
/// `replace`; the call of what it reads gives `o` to the stored function,
/// which the analysis must not prove.
#[test]
fn a_written_static_is_read_as_written() {
    let src = "class K { m(): number { return 1; } }
        const o: any = { k: new K() };
        const T: any = { Int16Array: Int16Array };
        const key: any = process.argv[2];
        (T.Int16Array as any)[key] = (x: any) => x;
        const s = /a(b)/.source.replace(\"b\", \"c\");
        const r = (T.Int16Array as any).replace(o);
        console.log(r, s.length);
        console.log(/*A*/ o.k.m());";
    let (ok, v) = proven(src, "m", true);
    assert!(!ok, "o.k.m() must stay unproven: {v}");
}
