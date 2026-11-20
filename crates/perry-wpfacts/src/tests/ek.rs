//! Rule EK: `Object.entries(o)` of a closed literal `o` yields exactly its
//! own enumerable String keys (ECMA-262 20.1.2.5, 7.3.23
//! EnumerableOwnProperties): a computed store under such a key is a named
//! store per key. The app's loop `for (const [k, v] of
//! Object.entries(classes)) globalThis[k] = v` then writes the global's
//! fields `A` and `B` only (R-GLOBAL: no any-key write), so a global binding
//! no key names keeps its value.
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    cfg.explain = std::env::var_os("EKX").is_some();
    crate::compute(&list, &cfg)
}

/// The verdict of the first site `.key` after `marker`, and the dump.
fn verdict(src: &str, key: &str, marker: &str) -> (String, String) {
    let pf = facts_of(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()));
    let v = format!("{:?}", s.verdict);
    eprintln!("EK {key}@{marker}: {v}");
    (v, pf.dump_text())
}

/// `before` runs, then the entries loop stores the pairs of `src_obj` on
/// the global object; the global `C` is read after it.
fn program(before: &str, src_obj: &str) -> String {
    format!(
        "class A {{ a = 1; }}
        class B {{ b = 2; }}
        class C {{ c = 3; }}
        const g = globalThis as Record<string, unknown>;
        const classes: any = {{ A, B }};
        (globalThis as any).C = C;
        {before}
        for (const [k, v] of Object.entries({src_obj})) g[k] = v;
        const c = new (globalThis as any).C();
        console.log(/*C*/ c.c);"
    )
}

/// Is `c.c` the field of `C` alone: the global `C` kept its value (no
/// stored class may be what `new` constructs)? An any-key store joins `A`
/// and `B` there, and `c.c` is an own-or-absent read of three classes.
fn c_kept(src: &str) -> (bool, String) {
    let (v, dump) = verdict(src, "c", "/*C*/");
    let kept = v.starts_with("T0Mono") || v.contains("Field") || v.contains("Slot");
    (kept, format!("{v}\n{dump}"))
}

/// RED: the loop over a closed literal writes `A` and `B` on the global
/// object (named stores; its done branch's `undefined` key writes
/// nothing): `C` keeps its value.
#[test]
fn ek_entries_named_global_stores() {
    let (ok, why) = c_kept(&program("", "classes"));
    assert!(ok, "the global C keeps its value: {why}");
}

/// RED: the keys of the entries are the literal's keys, a key written on
/// it by name before the loop among them.
#[test]
fn ek_entries_named_key_written() {
    let (ok, why) = c_kept(&program("classes.D = A;", "classes"));
    assert!(ok, "the global C keeps its value: {why}");
}

/// RED: a global function the program defines after the loop stays the
/// program's: its call constructs a `C`.
#[test]
fn ek_entries_later_global_function() {
    let src = program("", "classes").replace(
        "const c = new",
        "(globalThis as any).mk = function () { return new C(); };
        const d = (globalThis as any).mk();
        console.log(/*D*/ d.c);
        const c = new",
    );
    let (v, dump) = verdict(&src, "c", "/*D*/");
    assert!(
        v.starts_with("T0Mono") || v.contains("Field"),
        "d.c is C's field, got {v}\n{dump}"
    );
}

/// KEEP: where the keys are not known, the store is an any-key store as
/// before: every global binding may be one of the stored values.
#[test]
fn ek_keep_any_key() {
    for (before, obj) in [
        // a computed add before the loop widens the keys
        ("classes[String(process.argv[2])] = A;", "classes"),
        // the object escapes (stored on a host object): any key
        ("(process as any).x = classes;", "classes"),
        // a source whose keys are any String (a parsed object) beside it
        (
            "",
            "(Math.random() > 0.5 ? classes : JSON.parse(String(process.argv[2])))",
        ),
    ] {
        let (ok, why) = c_kept(&program(before, obj));
        assert!(!ok, "{before} / {obj}: an any-key store as before: {why}");
    }
}

/// KEEP: a computed add AFTER the loop still widens the keys the loop
/// stores under (the rule is monotone: `settle_for_in` looks again).
#[test]
fn ek_keep_later_computed_add() {
    let src = format!(
        "{}\nclasses[String(process.argv[2])] = B;",
        program("", "classes")
    );
    let (ok, why) = c_kept(&src);
    assert!(!ok, "a later computed add widens the keys: {why}");
}

/// KEEP (rule RP2): an `Iterator.from` result is an iterator model, never
/// an ordinary Array: its record takes the protocol arm, which is not held
/// back, and its pairs give any key.
#[test]
fn ek_keep_iterator_from_source() {
    let src = program(
        "const it = Iterator.from([[String(process.argv[2]), A]] as any);",
        "classes",
    )
    .replace("Object.entries(classes)", "it as any");
    let (ok, why) = c_kept(&src);
    assert!(!ok, "an Iterator.from source is stepped: {why}");
}

/// KEEP (rule RP2): a program write of `Array.prototype[Symbol.iterator]`
/// makes every Array take the protocol arm (`array_iteration_writes`):
/// the held tokens are released.
#[test]
fn ek_keep_array_iterator_written() {
    let src = program(
        "(Array.prototype as any)[Symbol.iterator] = function* () { yield [String(process.argv[2]), A]; };",
        "classes",
    );
    let (ok, why) = c_kept(&src);
    assert!(
        !ok,
        "a written Array @@iterator releases the held pairs: {why}"
    );
}
