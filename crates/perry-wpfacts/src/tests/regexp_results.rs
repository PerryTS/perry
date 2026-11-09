//! CHB7: the results of `RegExp.prototype[@@split]`, `exec` and `[@@match]`
//! are what their algorithms return (ECMA-262 22.2.6.14, 22.2.7.2,
//! 22.2.6.8) while the splitter and its `exec` are the intrinsics: an exact
//! Array of Strings (substrings) and captures (String or undefined); `exec`
//! null or an exact match record; `@@match` null, an Array of Strings, or
//! the match record. A program-written `exec` (on `RegExp.prototype` or a
//! subclass) or a program-supplied species constructor runs program code:
//! the captures are then what that code returns, unknown.
use super::*;

fn facts_of(src: &str, per_unit: bool) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    match per_unit {
        true => crate::compute_units(&list, &cfg),
        false => crate::compute(&list, &cfg),
    }
}

/// The verdict of the one site of `key` that follows `marker` in `src`.
fn verdict_at(pf: &ProgramFacts, src: &str, marker: &str, key: &str) -> Verdict {
    let at = src.find(marker).expect("marker") as u32;
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()))
        .verdict
        .clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// The site of `key` after `marker` is proven in both modes.
fn proves(src: &str, marker: &str, key: &str) {
    for per_unit in [false, true] {
        let v = verdict_at(&facts_of(src, per_unit), src, marker, key);
        assert!(proven(&v), "per_unit={per_unit} {marker} .{key}: {v:?}");
    }
}

fn unproven(src: &str, marker: &str, key: &str) {
    for per_unit in [false, true] {
        let v = verdict_at(&facts_of(src, per_unit), src, marker, key);
        assert!(!proven(&v), "per_unit={per_unit} {marker} .{key}: {v:?}");
    }
}

const SPLIT: &str =
    "function f(s: string) { const p = s.split(/(,)?\\s+/); return (p[1] ?? \"\").length; }\n\
                     console.log(f(\"a b\"), f(\"c\"));\n";

/// Split elements are substrings and intrinsic captures: String|undefined.
#[test]
fn split_element_or_empty_is_a_string() {
    proves(SPLIT, "(p[1]", "length");
}

/// `exec` of the intrinsic: null or a record whose captures are
/// String|undefined (`??` takes the undefined away).
#[test]
fn exec_capture_is_a_string_or_undefined() {
    proves(
        "function f(s: string) { const m = /(a)(b)?/.exec(s); if (m === null) return \"\";\n\
         return (m[2] ?? \"\").toUpperCase(); }\n\
         console.log(f(\"ab\"), f(\"a\"), f(\"z\"));\n",
        "(m[2]",
        "toUpperCase",
    );
}

/// `String.prototype.match` with a RegExp calls the intrinsic `@@match`.
#[test]
fn match_element_or_empty_is_a_string() {
    proves(
        "function f(s: string) { const k = s.match(/a+/g); return k === null ? 0 : (k[0] ?? \"\").length; }\n\
         console.log(f(\"aa\"), f(\"b\"));\n",
        "(k[0]",
        "length",
    );
}

/// `RegExp.prototype` is sealed (DEC 250, rt/lock.ts): `RegExp.prototype.exec = f`
/// throws, so the splitter still calls the intrinsic and the captures keep
/// their String|undefined fact. (Subclass and species `exec` below run.)
#[test]
fn sealed_prototype_exec_write_keeps_split_captures() {
    let src = format!(
        "(RegExp.prototype as any).exec = function (s: string): any {{ const r: any = [\"b\", {{ length: 9 }}]; r.index = 0; return r; }};\n{SPLIT}"
    );
    proves(&src, "(p[1]", "length");
}

/// A subclass is the species constructor of its instances; its `exec` runs.
#[test]
fn subclass_exec_makes_split_captures_unknown() {
    unproven(
        "class R extends RegExp { exec(s: string): any { const r: any = [\"b\", { length: 9 }]; r.index = 0; return r; } }\n\
         function f(s: string, re: RegExp) { const p = s.split(re); return (p[1] ?? \"\").length; }\n\
         console.log(f(\"a b\", new R(\"(,)?\\\\s+\")), f(\"c\", /x/));\n",
        "(p[1]",
        "length",
    );
}

/// A species constructor that makes a plain object: its `exec` runs.
#[test]
fn species_constructor_makes_split_captures_unknown() {
    unproven(
        "class S { constructor(r: any, fl: string) { return { lastIndex: 0, exec(s: string): any { return null; }, flags: fl } as any; } }\n\
         class R extends RegExp { static get [Symbol.species]() { return S; } }\n\
         function f(s: string, re: RegExp) { const p = s.split(re); return (p[1] ?? \"\").length; }\n\
         console.log(f(\"a b\", new R(\"(,)?\\\\s+\")), f(\"c\", /x/));\n",
        "(p[1]",
        "length",
    );
}

/// A subclass `exec` is what `@@match` calls.
#[test]
fn subclass_exec_makes_match_elements_unknown() {
    unproven(
        "class R extends RegExp { exec(s: string): any { const r: any = [{ length: 9 }]; r.index = 0; return r; } }\n\
         function f(s: string, re: RegExp) { const k = s.match(re); return k === null ? 0 : (k[0] ?? \"\").length; }\n\
         console.log(f(\"a\", new R(\"a\")), f(\"c\", /x/));\n",
        "(k[0]",
        "length",
    );
}

/// `RegExp.prototype.exec = f` throws (sealed): a direct `exec` of a
/// literal is still the intrinsic and keeps its result facts.
#[test]
fn sealed_prototype_exec_write_keeps_exec_result() {
    proves(
        "(RegExp.prototype as any).exec = function (s: string): any { return [{ length: 9 }, 1]; };\n\
         function f(s: string) { const m = /(a)/.exec(s); return m === null ? 0 : (m[0] ?? \"\").length; }\n\
         console.log(f(\"a\"));\n",
        "(m[0]",
        "length",
    );
}

/// Named control: an own-key `exec` written on an escaped regexp instance
/// is a program function, so its result is unknown (the caller's path, DESIGN 2).
#[test]
fn own_key_exec_write_on_an_instance_makes_exec_result_unknown() {
    unproven(
        "function f(s: string) { const re: any = /(a)/; re.exec = function (s: string): any { return (globalThis as any)[process.argv[2]](s); };\n\
         const m = re.exec(s); return m === null ? 0 : (m[0] ?? \"\").length; }\n\
         console.log(f(\"a\"));\n",
        "(m[0]",
        "length",
    );
}

/// The member read on a capture a written `exec` returned: the capture is
/// that program object, never a primitive or a primitive container.
#[test]
fn written_exec_split_capture_member_is_unknown() {
    unproven(
        "(RegExp.prototype as any).exec = function (this: RegExp, s: string): any {\n\
         if (this.lastIndex !== 1) return null; this.lastIndex = 2; const r: any = [\"b\", { v: 7 }]; r.index = 1; return r; };\n\
         function f(s: string, re: RegExp) { const parts: any[] = s.split(re); const x = parts[1]; if (typeof x === \"object\" && x !== null) return x.v; return 0; }\n\
         console.log(f(\"abc\", /b/));\n",
        "x.v",
        "v",
    );
}

/// The same through a subclass `exec`.
#[test]
fn subclass_exec_split_capture_member_is_unknown() {
    unproven(
        "class R extends RegExp { exec(s: string): any {\n\
         if (this.lastIndex !== 1) return null; this.lastIndex = 2; const r: any = [\"b\", { v: 7 }]; r.index = 1; return r; } }\n\
         function f(s: string, re: RegExp) { const parts: any[] = s.split(re); const x = parts[1]; if (typeof x === \"object\" && x !== null) return x.v; return 0; }\n\
         console.log(f(\"abc\", new R(\"b\")));\n",
        "x.v",
        "v",
    );
}

/// A capture a written `exec` returned may be a program function: a call of
/// it runs that function.
#[test]
fn written_exec_split_capture_call_is_unknown() {
    let src = "function h(): string { return \"h\"; }\n\
         (RegExp.prototype as any).exec = function (this: RegExp, s: string): any {\n\
         if (this.lastIndex !== 1) return null; this.lastIndex = 2; const r: any = [\"b\", h]; r.index = 1; return r; };\n\
         function f(s: string, re: RegExp) { const parts: any[] = s.split(re); const x = parts[1]; if (typeof x === \"function\") return x(); return 0; }\n\
         console.log(f(\"abc\", /b/));\n";
    unproven(src, "return x()", "()");
}

/// Two modules: what `tests` sees of the program, per unit or whole.
fn facts_of_modules(mods: &[(&str, &str)], per_unit: bool) -> ProgramFacts {
    let hirs: Vec<(String, perry_hir::Module)> = mods
        .iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect();
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    match per_unit {
        true => crate::compute_units(&list, &cfg),
        false => crate::compute(&list, &cfg),
    }
}

/// CHB13/CHB14: a RegExp object holds `lastIndex` as an own non-configurable
/// data property (22.2.3.1 RegExpAlloc), and `new RegExp(p, f)` reads only
/// `p[@@match]`, `p.source`, `p.flags` and ToStrings its operands
/// (22.2.4.1): a store into ⊤ under `lastIndex` reaches neither the
/// `lastIndex` reads of `@@split` / `exec` nor the species construct, so
/// another module's split elements stay Strings (chess tests/run.ts: whole
/// merged a ⊤ and a String `fen`). Per unit is never stronger than whole;
/// a cross-unit `string` parameter's split is not proven per unit today,
/// with or without the store.
#[test]
fn store_through_top_under_last_index_keeps_split_elements() {
    let b = "export function cmd(raw: string): string { const w = raw.trim().split(/\\s+/); return (w[0] ?? \"\").toLowerCase(); }\n";
    let main = [
        "import { cmd } from \"./b\";\nconst o: any = (globalThis as any)[process.argv[2]];\no.lastIndex = 1;\nconsole.log(cmd(\"A b\"));\n",
        "import { cmd } from \"./b\";\nfor (const o of [{ n: 1 }, { n: 2 }]) (o as any).lastIndex = 1;\nconsole.log(cmd(\"A b\"));\n",
    ];
    for m in main {
        let pf = facts_of_modules(&[("b", b), ("main", m)], false);
        let v = pf
            .sites
            .iter()
            .find(|s| s.key == "toLowerCase")
            .unwrap_or_else(|| panic!("toLowerCase site\n{}", pf.dump_text()))
            .verdict
            .clone();
        assert!(proven(&v), "whole .toLowerCase after {m:?}: {v:?}");
    }
}

/// What the program stores into a regex's own `lastIndex` is read back
/// (the own field keeps it beside the birth Number; no prototype read).
#[test]
fn stored_last_index_is_read_back() {
    let src = "class K { k = 1; }\nconst r: any = /a/g;\nr.lastIndex = new K();\nconsole.log(r.lastIndex.k);\n";
    let v = verdict_at(&facts_of(src, false), src, "console.log(r.lastIndex", "k");
    assert!(proven(&v), "whole r.lastIndex.k: {v:?}");
}
