//! The CommonJS plumbing's own reads are proven one-load sites (lane CJM).
//!
//! A real three-module program goes through the real wrapper and the real
//! support module (`support.rs`), is lowered, and is solved whole-program.
//! Every read the wrapper performs on its own objects (`require.cache[<path>]`,
//! a record's `exports`, `link`'s `parent` / `children`) must be proven, and
//! no wrapper site may read a `globalThis` property or a property of a
//! builtin `require`. Each of these fails when the corresponding object stops
//! being compiler-owned (the record as a builtin object, the parent link on
//! `globalThis`, the cache as an ordinary map).

use std::path::{Path, PathBuf};

use perry_wpfacts::{FactsConfig, ProgramFacts, Verdict};

use super::support;
use super::wrap::wrap_commonjs_with_body_offset;

/// `(file name, CommonJS source)`, dependencies before dependents; the last
/// one is the entry.
fn solve(sources: &[(&str, &str)]) -> (tempfile::TempDir, Vec<String>, ProgramFacts) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical tempdir");
    let paths: Vec<PathBuf> = sources
        .iter()
        .map(|(name, src)| {
            let p = root.join(name);
            std::fs::write(&p, src).expect("write module");
            p
        })
        .collect();
    let keys: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let support_path = root.join("perry-cjs.mjs");
    let support_src = support::source(&keys, true);
    let mut texts = vec![(support_path.clone(), support_src)];
    for (i, (_, src)) in sources.iter().enumerate() {
        let entry = i + 1 == sources.len();
        let (wrapped, _) = wrap_commonjs_with_body_offset(src, &paths[i], None, entry, None);
        texts.push((paths[i].clone(), wrapped));
    }
    let resolve = |source: &str| -> Option<String> {
        if source == support::SPECIFIER {
            return Some(support_path.to_string_lossy().into_owned());
        }
        let rel = source.strip_prefix("./")?;
        Some(root.join(rel).to_string_lossy().into_owned())
    };
    let mut modules = Vec::new();
    for (path, text) in &texts {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let ast = perry_parser::parse_typescript(text, &name).expect("wrapped source parses");
        let mut hir =
            perry_hir::lower_module(&ast, &name, &path.to_string_lossy()).expect("lowers");
        for imp in &mut hir.imports {
            if imp.resolved_path.is_none() {
                imp.resolved_path = resolve(&imp.source);
            }
        }
        modules.push((path.to_string_lossy().into_owned(), hir));
    }
    let list: Vec<(String, &perry_hir::Module)> =
        modules.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = perry_wpfacts::compute(&list, &cfg);
    let names = modules.iter().map(|(p, _)| p.clone()).collect();
    (dir, names, pf)
}

/// The facts' index of the support module (the solve orders modules itself).
fn support_index(pf: &ProgramFacts) -> u32 {
    pf.dump_text()
        .lines()
        .filter_map(|l| l.strip_prefix("M "))
        .find_map(|l| {
            let (i, path) = l.split_once(' ')?;
            path.ends_with("perry-cjs.mjs")
                .then(|| i.parse().ok())
                .flatten()
        })
        .expect("the support module is one of the solved modules")
}

fn proven(v: &Verdict) -> bool {
    matches!(v, Verdict::T0Mono | Verdict::T0Set)
}

const B: &str = "exports.name = 'b';\nexports.twice = function (n) { return n * 2; };\n";
const C: &str = "const b = require('./b.js');\nexports.name = 'c' + b.name;\n";
const A: &str = "const b = require('./b.js');\nconst c = require('./c.js');\nconsole.log(b.twice(2), c.name, module.exports === exports);\n";

#[test]
fn the_wrappers_own_reads_are_proven_one_load_sites() {
    let (dir, names, pf) = solve(&[("b.js", B), ("c.js", C), ("a.js", A)]);
    let root = dir
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let dump = pf.dump_text();
    let support = support_index(&pf);
    let wrapped = |s: &perry_wpfacts::SiteFacts| s.module != support;
    // `require.cache["<path>"]`: a declared key of the cache, one slot.
    let cache_reads: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| wrapped(s) && s.key.starts_with(&root))
        .collect();
    // a.js requires b and c, c.js requires b: three static edges, each
    // reading the live entry and, after a reload, the new one.
    assert_eq!(cache_reads.len(), 6, "{names:?}\n{dump}");
    for s in &cache_reads {
        assert!(
            proven(&s.verdict),
            "cache read {} not proven\n{dump}",
            s.key
        );
        assert!(
            s.receivers
                .iter()
                .all(|r| r.contains("__PerryDeclaredKeys_")),
            "cache read {} on {:?}\n{dump}",
            s.key,
            s.receivers
        );
    }
    // A record's `exports` (the wrapper's `var exports`, its return, a cached
    // child's exports): the record literal's class, one slot.
    let exports_reads: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| wrapped(s) && s.key == "exports" && !s.is_call)
        .filter(|s| s.receivers.iter().any(|r| r.contains("__AnonShape_")))
        .collect();
    assert!(exports_reads.len() >= 6, "{dump}");
    for s in &exports_reads {
        assert!(proven(&s.verdict), "record exports read not proven\n{dump}");
    }
    // `link`: Node's updateChildren over records.
    for key in ["parent", "children"] {
        let reads: Vec<_> = pf
            .sites
            .iter()
            .filter(|s| s.module == support && s.key == key && !s.is_call)
            .collect();
        assert!(!reads.is_empty(), "no `{key}` read in link\n{dump}");
        for s in reads {
            assert!(proven(&s.verdict), "link reads `{key}` unproven\n{dump}");
        }
    }
    // Nothing the wrapper reads is a `globalThis` property or a property of
    // a builtin `require`.
    for s in pf.sites.iter().filter(|s| wrapped(s)) {
        assert!(
            !s.receivers
                .iter()
                .any(|r| r == "builtin:GlobalThis" || r == "builtin:Require"),
            "wrapper site `{}` reads a builtin receiver {:?}\n{dump}",
            s.key,
            s.receivers
        );
    }
}

/// Every record shares one key sequence, so one slot per key. A key one
/// module adds to its own record is not a birth key: it must not be read at
/// another key's slot.
#[test]
fn a_key_one_record_adds_is_not_read_at_a_shared_slot() {
    let b = "module.extra = 1;\nexports.extra = module.extra;\n";
    let (_dir, _names, pf) = solve(&[("b.js", b), ("c.js", C), ("a.js", A)]);
    let dump = pf.dump_text();
    let support = support_index(&pf);
    let extra: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module != support && s.key == "extra" && !s.is_call)
        .filter(|s| s.receivers.iter().any(|r| r.contains("__AnonShape_")))
        .collect();
    assert!(!extra.is_empty(), "{dump}");
    for s in extra {
        assert!(
            !proven(&s.verdict),
            "an added record key was proven\n{dump}"
        );
    }
    // The records' shared keys stay proven around it.
    assert!(
        pf.sites
            .iter()
            .filter(|s| s.module != support && s.key == "exports" && !s.is_call)
            .filter(|s| s.receivers.iter().any(|r| r.contains("__AnonShape_")))
            .all(|s| proven(&s.verdict)),
        "{dump}"
    );
}

#[test]
fn the_support_module_is_imported_first() {
    let path = Path::new("/tmp/cjm/x.js");
    let (wrapped, _) = wrap_commonjs_with_body_offset("exports.x = 1;\n", path, None, false, None);
    let first = wrapped
        .lines()
        .find(|l| l.trim_start().starts_with("import "))
        .expect("an import");
    assert!(first.contains(support::SPECIFIER), "{first}");
    assert!(!wrapped.contains("globalThis.__perry_cjs"), "{wrapped}");
}

/// Step 2: a module's born `exports` carries the names it assigns as
/// declared keys, so its own reads and the export snapshots are slots.
#[test]
fn the_born_exports_object_has_the_module_s_export_contract() {
    let b = "exports.name = 'b';\nexports.twice = function (n) { return n * 2; };\nconsole.log(exports.name);\n";
    let (_dir, _names, pf) = solve(&[("b.js", b), ("c.js", C), ("a.js", A)]);
    let dump = pf.dump_text();
    let reads: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "name" && !s.is_call)
        .filter(|s| {
            s.receivers
                .iter()
                .all(|r| r.contains("__PerryDeclaredKeysObject_"))
        })
        .collect();
    assert!(!reads.is_empty(), "{dump}");
    for s in reads {
        assert!(proven(&s.verdict), "an exports read is unproven\n{dump}");
    }
}

/// (a) The contract is the born object's only: after `module.exports = X`,
/// a read through `module.exports` may see `X`, which lacks the key.
#[test]
fn the_exports_contract_does_not_follow_a_replacement() {
    let b = "exports.a = 1;\nmodule.exports = { other: 2 };\nconsole.log(module.exports.a);\n";
    let (_dir, _names, pf) = solve(&[("b.js", b), ("c.js", C), ("a.js", A)]);
    let dump = pf.dump_text();
    let reads: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "a" && !s.is_call)
        .filter(|s| {
            s.receivers
                .iter()
                .any(|r| !r.contains("__PerryDeclaredKeysObject_"))
        })
        .collect();
    assert!(!reads.is_empty(), "{dump}");
    for s in reads {
        assert!(
            !proven(&s.verdict),
            "a replaced exports read was proven\n{dump}"
        );
    }
}

/// (c) A declared key that becomes an accessor (esbuild's `__export`
/// defines getters) is never read as a data slot.
#[test]
fn a_declared_export_redefined_as_an_accessor_is_not_a_slot() {
    let b = "exports.v = 1;\nObject.defineProperty(exports, 'v', { get() { return 2; } });\nconsole.log(exports.v);\n";
    let (_dir, _names, pf) = solve(&[("b.js", b), ("c.js", C), ("a.js", A)]);
    let dump = pf.dump_text();
    let reads: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "v" && !s.is_call)
        .filter(|s| {
            s.receivers
                .iter()
                .any(|r| r.contains("__PerryDeclaredKeysObject_"))
        })
        .collect();
    assert!(!reads.is_empty(), "{dump}");
    for s in reads {
        assert!(
            !proven(&s.verdict),
            "an accessor export was read as a slot\n{dump}"
        );
    }
}

/// (b) A computed store may write a declared key: it probes the key index
/// and lands in the slot, so a static read stays a slot read with the hole
/// compare (and the runtime test pins the value).
#[test]
fn a_computed_exports_store_keeps_declared_reads_sound() {
    let b = "const k = ['x', 'y'][0];\nexports[k] = 'computed';\nexports.x = exports.x + '!';\n";
    let (_dir, _names, pf) = solve(&[("b.js", b), ("c.js", C), ("a.js", A)]);
    let dump = pf.dump_text();
    assert!(
        pf.sites.iter().any(|s| s.key == "x"
            && s.receivers
                .iter()
                .any(|r| r.contains("__PerryDeclaredKeysObject_"))),
        "{dump}"
    );
}
