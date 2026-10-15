//! Same compiled C fixtures and sabotage assertions as self_test.py.
//! Binaries are checked in so cargo test needs LLVM, but never clang/a cross SDK.
use perry_gc_effects::*;
use std::path::{Path, PathBuf};
const RULES: &str = include_str!("fixtures/seeds.txt");
type Fixture = (&'static str, &'static [u8], &'static [u8], &'static [u8]);
const FORMATS: [Fixture; 3] = [
    (
        "elf-x86_64",
        include_bytes!("fixtures/libfixture_elf-x86_64.a"),
        include_bytes!("fixtures/libfixture_elf-x86_64-planted.a"),
        include_bytes!("fixtures/libhot_text_elf-x86_64.a"),
    ),
    (
        "macho-arm64",
        include_bytes!("fixtures/libfixture_macho-arm64.a"),
        include_bytes!("fixtures/libfixture_macho-arm64-planted.a"),
        include_bytes!("fixtures/libhot_text_macho-arm64.a"),
    ),
    (
        "coff-x86_64",
        include_bytes!("fixtures/libfixture_coff-x86_64.a"),
        include_bytes!("fixtures/libfixture_coff-x86_64-planted.a"),
        include_bytes!("fixtures/libhot_text_coff-x86_64.a"),
    ),
];
fn archive(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}
#[test]
fn three_formats_classification_and_planted_violations() {
    let tmp = tempfile::tempdir().unwrap();
    let rules = load_rules(RULES).unwrap();
    let root = workspace_root();
    let mut baseline = None;
    for (tag, normal, planted, hot) in FORMATS {
        let lib = archive(tmp.path(), &format!("{tag}.a"), normal);
        let graph = build_graph(std::slice::from_ref(&lib)).unwrap();
        let res = classify(&graph, &rules, None).unwrap();
        res.validate().unwrap();
        let expected = [
            ("js_back_hot_text", Class::Leaf, false),
            ("leaf_helper", Class::Leaf, false),
            ("libc_helper", Class::Leaf, false),
            ("switch_helper", Class::Leaf, false),
            ("alloc_helper", Class::AllocOnly, false),
            ("throw_helper", Class::ThrowOnly, true),
            ("reenter_helper", Class::Reenters, true),
            ("chain_helper", Class::Reenters, true),
            ("indirect_helper", Class::Reenters, true),
            ("extern_helper", Class::Reenters, true),
            ("poll_helper", Class::Reenters, false),
            ("unwind_helper", Class::Reenters, true),
        ];
        for (name, class, throws) in expected {
            assert_eq!(
                res.table.get(name),
                Some(&Entry { class, throws }),
                "{tag}: {name}"
            );
        }
        let selected: Vec<_> = expected.iter().map(|(n, _, _)| res.table[*n]).collect();
        if let Some(base) = &baseline {
            assert_eq!(&selected, base, "{tag} format disagreement");
        } else {
            baseline = Some(selected);
        }
        assert!(
            graph
                .nodes
                .iter()
                .any(|n| n.names.iter().any(|s| s == "switch_helper") && n.switches > 0),
            "{tag}: switch assertion is vacuous"
        );
        let native = classify(&graph, &rules, Some(&root)).unwrap();
        assert_eq!(native.table["js_back_hot_text"].class, Class::Leaf);
        let mut cj = RULES.to_owned();
        cj.push_str("collector exact fixture_collect_js -- fixture collector that runs JS\n");
        let cj = classify(&graph, &load_rules(&cj).unwrap(), None).unwrap();
        assert_eq!(
            cj.table["gc_reaches_js_helper"],
            Entry {
                class: Class::AllocOnly,
                throws: false
            }
        );
        let hot = archive(tmp.path(), &format!("hot-{tag}.a"), hot);
        let hg = build_graph(&[hot]).unwrap();
        let hr = classify(&hg, &load_rules(SEEDS).unwrap(), Some(&root)).unwrap();
        assert_eq!(
            hr.table["js_back_hot_text"].class,
            Class::Leaf,
            "{tag}: real C production body"
        );
        let plant = archive(tmp.path(), &format!("plant-{tag}.a"), planted);
        let pg = build_graph(&[plant]).unwrap();
        let pr = classify(&pg, &rules, None).unwrap();
        assert_eq!(
            pr.table["leaf_helper"],
            Entry {
                class: Class::Reenters,
                throws: true
            }
        );
        let pn = classify(&pg, &rules, Some(&root)).unwrap();
        for (old, new, name) in [
            (&res.table, &pr.table, "leaf_helper"),
            (&native.table, &pn.table, "js_back_hot_text"),
        ] {
            let (unsafe_drift, _) = compare(old, new);
            assert!(
                unsafe_drift.iter().any(|d| d.name == name && !d.throws),
                "{tag}: planted class drift not detected"
            );
            assert!(
                unsafe_drift.iter().any(|d| d.name == name && d.throws),
                "{tag}: planted throws drift not detected"
            );
        }
        for mode in [Mode::L2, Mode::Throws] {
            let path = pr.witness_path(&pg, "leaf_helper", mode);
            assert!(
                path.last().unwrap().contains("js_proxy_get"),
                "{tag}: {path:?}"
            );
        }
        let dead = load_rules(
            &(RULES.to_owned()
                + "js exact no_such_symbol_anywhere -- deliberately dead fixture seed\n"),
        )
        .unwrap();
        let dr = classify(&graph, &dead, None).unwrap();
        assert!(!dr.stats.dead_rules.is_empty());
        assert!(dr.validate().unwrap_err().to_string().contains("NO node"));
        let forbid = load_rules(
            &(RULES.to_owned()
                + "forbid exact unknown_external -- deliberately violated fixture premise\n"),
        )
        .unwrap();
        let fr = classify(&graph, &forbid, None).unwrap();
        assert!(!fr.stats.forbidden_calls.is_empty());
        assert!(fr.validate().unwrap_err().to_string().contains("forbid"));
    }
}
#[test]
fn drift_absence_and_agreement_gate() {
    fn table(rows: &[(&str, Class, bool)]) -> Table {
        rows.iter()
            .map(|(n, class, throws)| {
                (
                    n.to_string(),
                    Entry {
                        class: *class,
                        throws: *throws,
                    },
                )
            })
            .collect()
    }
    let c = table(&[
        ("a", Class::Leaf, false),
        ("b", Class::Reenters, true),
        ("d", Class::Leaf, false),
    ]);
    let f = table(&[
        ("b", Class::Leaf, false),
        ("c", Class::Leaf, false),
        ("d", Class::Leaf, true),
    ]);
    let (u, s) = compare(&c, &f);
    assert_eq!(
        u.iter()
            .filter(|d| !d.throws)
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>(),
        ["a"]
    );
    assert_eq!(
        s.iter()
            .filter(|d| !d.throws)
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>(),
        ["b", "c"]
    );
    assert_eq!(
        u.iter()
            .filter(|d| d.throws)
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>(),
        ["a", "d"]
    );
    assert_eq!(
        s.iter()
            .filter(|d| d.throws)
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>(),
        ["b", "c"]
    );
    let mut fake = Classification::default();
    fake.table = table(&[
        ("a", Class::Leaf, true),
        ("b", Class::AllocOnly, false),
        ("c", Class::Reenters, true),
    ]);
    assert_eq!(fake.agreement_violations(), ["a"]);
    assert!(fake.validate().is_err());
}
#[test]
fn cli_rejects_sabotaged_table_dead_seed_and_forbid() {
    let tmp = tempfile::tempdir().unwrap();
    let root = workspace_root();
    let bin = env!("CARGO_BIN_EXE_perry-gc-effects");
    let rules = tmp.path().join("rules.txt");
    std::fs::write(&rules, RULES).unwrap();
    let lib = archive(tmp.path(), "baseline.a", FORMATS[0].1);
    let plant = archive(tmp.path(), "planted.a", FORMATS[0].2);
    let table = tmp.path().join("effects.tsv");
    let run = |command: &str, input: &Path| {
        std::process::Command::new(bin)
            .arg("--root")
            .arg(&root)
            .arg("--rules")
            .arg(&rules)
            .arg(command)
            .args(["--target", "linux-x86_64", "--min-symbols", "1"])
            .arg(if command == "generate" {
                "--out"
            } else {
                "--table"
            })
            .arg(&table)
            .arg(input)
            .output()
            .unwrap()
    };
    let generated = run("generate", &lib);
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let check = run("check", &plant);
    assert!(!check.status.success());
    assert!(String::from_utf8_lossy(&check.stderr).contains("UNSAFE"));
    for extra in [
        "js exact no_such_symbol_anywhere -- deliberately dead fixture seed\n",
        "forbid exact unknown_external -- deliberately violated fixture premise\n",
    ] {
        std::fs::write(&rules, RULES.to_owned() + extra).unwrap();
        assert!(
            !run("generate", &lib).status.success(),
            "planted hard-check violation succeeded"
        );
    }
}

#[test]
fn complete_tsv_bytes_match_python_goldens_in_three_formats() {
    let tmp = tempfile::tempdir().unwrap();
    let root = workspace_root();
    let rules = load_rules(RULES).unwrap();
    let goldens = [
        (
            "elf-x86_64",
            "linux-x86_64",
            include_str!("fixtures/elf-x86_64.tsv"),
            include_str!("fixtures/elf-x86_64-planted.tsv"),
        ),
        (
            "macho-arm64",
            "macos-aarch64",
            include_str!("fixtures/macho-arm64.tsv"),
            include_str!("fixtures/macho-arm64-planted.tsv"),
        ),
        (
            "coff-x86_64",
            "windows-x86_64",
            include_str!("fixtures/coff-x86_64.tsv"),
            include_str!("fixtures/coff-x86_64-planted.tsv"),
        ),
    ];
    for ((tag, normal, planted, _), (_, target, golden, plant_golden)) in
        FORMATS.into_iter().zip(goldens)
    {
        for (suffix, bytes, want) in [("", normal, golden), ("-planted", planted, plant_golden)] {
            let path = archive(tmp.path(), &format!("libfixture_{tag}{suffix}.a"), bytes);
            let (g, res) = analyze(std::slice::from_ref(&path), &rules, &root).unwrap();
            assert!(!g.nodes.is_empty());
            let out = tmp.path().join("fresh.tsv");
            write_table(&res.table, target, &[path], &out).unwrap();
            assert_eq!(std::fs::read_to_string(out).unwrap(), want, "{tag}{suffix}");
        }
    }
}
