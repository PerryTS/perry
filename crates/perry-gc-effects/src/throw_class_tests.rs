use super::*;
use crate::table::{PassSource, ThrowClass};

fn graph(names: &[&str], edges: &[(usize, usize)]) -> Graph {
    let mut nodes: Vec<_> = names
        .iter()
        .map(|name| {
            let mut n = Node::new(
                "runtime.perry_runtime.hash.rcgu.o".into(),
                ".text".into(),
                0,
                crate::instructions::Format::Elf,
            );
            n.names.push((*name).into());
            n
        })
        .collect();
    for &(a, b) in edges {
        nodes[a].calls.insert(names[b].into());
    }
    let globals = nodes
        .iter()
        .map(|n| (n.obj.clone(), n.names[0].clone()))
        .collect();
    Graph::new(nodes, globals).unwrap()
}

#[test]
fn four_throw_classes_and_neutral_caught_and_program_body() {
    let g = graph(
        &[
            "entry_none",
            "entry_core",
            "entry_passes",
            "entry_open",
            "perry_runtime::exception::throw_leaf::throw_core",
            "perry_runtime::exception::throw_leaf::throw_program_value",
            "perry_runtime::exception::throw_leaf::throw_open",
            "perry_runtime::exception::throw_leaf::rethrow_caught",
            "program_body",
        ],
        &[(0, 7), (0, 8), (1, 4), (2, 5), (3, 6), (8, 6)],
    );
    let decls = vec![crate::throw_class::Declaration::own("entry_passes", 0)];
    let mut cut = vec![false; 9];
    cut[8] = true;
    let r = crate::throw_class::solve(&g, &vec![vec![]; 9], &cut, &vec![None; 9], &decls).unwrap();
    assert_eq!(
        &r.class[..4],
        &[
            ThrowClass::None,
            ThrowClass::Core,
            ThrowClass::Passes,
            ThrowClass::Open
        ]
    );
    assert_eq!(
        r.passes[2],
        [PassSource {
            source: None,
            argument: 0
        }]
        .into()
    );
}

#[test]
fn missing_passes_names_the_entry_even_when_also_open() {
    let g = graph(
        &[
            "entry_without_declaration",
            "perry_runtime::exception::throw_leaf::throw_program_value",
            "perry_runtime::exception::throw_leaf::throw_open",
        ],
        &[(0, 1), (0, 2)],
    );
    let err = crate::throw_class::solve(&g, &vec![vec![]; 3], &[false; 3], &vec![None; 3], &[])
        .unwrap_err();
    assert!(
        err.to_string().contains("entry_without_declaration"),
        "{err:#}"
    );
}

#[test]
fn own_operands_do_not_cross_calls_but_stored_operands_do() {
    let g = graph(
        &[
            "caller",
            "producer",
            "perry_runtime::exception::throw_leaf::throw_program_value",
        ],
        &[(0, 1), (1, 2)],
    );
    let own = crate::throw_class::Declaration::own("producer", 0);
    let run = |ds: &[_]| {
        crate::throw_class::solve(&g, &vec![vec![]; 3], &[false; 3], &vec![None; 3], ds).unwrap()
    };
    let r = run(&[own.clone()]);
    assert_eq!(r.class[0], ThrowClass::Open);
    assert_eq!(r.own_unmapped[0].len(), 1);
    assert_eq!(r.class[1], ThrowClass::Passes);
    let mut stored = own.clone();
    stored.operands[0].source = Some("Store.put".into());
    let r = run(&[stored]);
    assert_eq!(r.class[0], ThrowClass::Passes);
    assert_eq!(
        r.passes[0],
        [PassSource {
            source: Some("Store.put".into()),
            argument: 0
        }]
        .into()
    );
    let r = run(&[own, crate::throw_class::Declaration::own("caller", 1)]);
    assert_eq!(r.class[0], ThrowClass::Passes);
    assert_eq!(
        r.passes[0],
        [PassSource {
            source: None,
            argument: 1
        }]
        .into()
    );
}

#[test]
fn unresolved_indirect_is_open_and_delegated_edges_are_walked() {
    let g = graph(
        &[
            "caller",
            "callback",
            "perry_runtime::exception::throw_leaf::throw_core",
        ],
        &[(1, 2)],
    );
    let unresolved = vec![Some("indirect_call".into()), None, None];
    let r = crate::throw_class::solve(&g, &vec![vec![]; 3], &[false; 3], &unresolved, &[]).unwrap();
    assert_eq!(r.class[0], ThrowClass::Open);
    let r = crate::throw_class::solve(
        &g,
        &[vec![], vec![0], vec![]],
        &[false; 3],
        &vec![None; 3],
        &[],
    )
    .unwrap();
    assert_eq!(r.class[0], ThrowClass::Core);
}

#[test]
fn raw_entry_call_fails_graph_check() {
    let g = graph(&["scratch_runtime_raw_call", "js_throw"], &[(0, 1)]);
    let err = crate::throw_class::solve(&g, &vec![vec![]; 2], &[false; 2], &vec![None; 2], &[])
        .unwrap_err();
    assert!(
        err.to_string().contains("scratch_runtime_raw_call"),
        "{err:#}"
    );
}

#[test]
fn unused_passes_is_rejected() {
    let g = graph(&["unused_passes"], &[]);
    let err = crate::throw_class::solve(
        &g,
        &[vec![]],
        &[false],
        &[None],
        &[crate::throw_class::Declaration::own("unused_passes", 0)],
    )
    .unwrap_err();
    assert!(err.to_string().contains("unused_passes"), "{err:#}");
}

#[test]
fn coverage_walks_internal_edges_but_stops_at_program_cuts() {
    let mut g = graph(
        &[
            "exported_caller",
            "local_body",
            "perry_runtime::exception::throw_leaf::throw_program_value",
            "program_body",
        ],
        &[(0, 1), (1, 2)],
    );
    g.by_global.remove("local_body");
    let error = crate::throw_class::solve(&g, &vec![vec![]; 4], &[false; 4], &vec![None; 4], &[])
        .unwrap_err();
    assert!(error.to_string().contains("exported_caller"), "{error:#}");
    let mut g = graph(
        &[
            "exported_caller",
            "local_body",
            "perry_runtime::exception::throw_leaf::throw_program_value",
            "program_body",
        ],
        &[(0, 3), (3, 1), (1, 2)],
    );
    g.by_global.remove("local_body");
    let result = crate::throw_class::solve(
        &g,
        &vec![vec![]; 4],
        &[false, false, false, true],
        &vec![None; 4],
        &[],
    )
    .unwrap();
    assert_eq!(result.class[0], ThrowClass::None);
}

#[test]
fn a_folded_alias_does_not_inherit_another_entrys_own_argument() {
    let mut g = graph(
        &[
            "producer",
            "perry_runtime::exception::throw_leaf::throw_program_value",
        ],
        &[(0, 1)],
    );
    g.nodes[0].names.push("alias".into());
    g.by_global.insert("alias".into(), vec![0]);
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("crates/perry-runtime/src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"),"crate::fn_info!(#[realm(throws=passes(0))] declare #[no_mangle] pub fn producer(value:f64)->f64 {value});").unwrap();
    let rules = crate::load_rules(
        "throw prefix perry_runtime::exception::throw_leaf:: -- test typed leaf transport\n",
    )
    .unwrap();
    let result = classify(&g, &rules, Some(tmp.path())).unwrap();
    assert_eq!(result.table["producer"].throw_class, ThrowClass::Passes);
    assert_eq!(result.table["producer"].passes_text(), "self:0");
    assert_eq!(result.table["alias"].throw_class, ThrowClass::Open);
    assert!(result.table["alias"].passes.is_empty());
}

#[test]
fn native_c_runtime_objects_are_covered_by_the_raw_entry_check() {
    let mut g = graph(&["native_runtime_caller", "js_throw"], &[(0, 1)]);
    for n in &mut g.nodes {
        n.obj = "0abcd-hot_text.o".into();
    }
    let stems = ["hot_text".to_owned()].into_iter().collect();
    let owned: Vec<_> = g
        .nodes
        .iter()
        .map(|n| crate::table::perry_object(&n.obj, &stems))
        .collect();
    let err = crate::throw_class::solve_owned(
        &g,
        &vec![vec![]; 2],
        &[false; 2],
        &vec![None; 2],
        &[],
        &owned,
        &[true; 2],
    )
    .unwrap_err();
    assert!(err.to_string().contains("native_runtime_caller"), "{err:#}");
}

#[test]
fn coverage_uses_the_exact_runtime_sidecar_entry_roots() {
    let make = |edges: &[(usize, usize)]| {
        let mut g = graph(
            &[
                "foreign_export",
                "perry_runtime::exception::throw_leaf::throw_program_value",
                "runtime_entry",
            ],
            edges,
        );
        g.nodes[0].obj = "fixture.foreign.hash.rcgu.o".into();
        g
    };
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("crates/perry-runtime/src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "").unwrap();
    let rules = crate::load_rules(
        "throw prefix perry_runtime::exception::throw_leaf:: -- test native-origin leaf\n",
    )
    .unwrap();
    let result = classify(&make(&[(0, 1)]), &rules, Some(tmp.path()));
    assert!(
        result.is_ok(),
        "a non-runtime export is not a sidecar entry: {:?}",
        result.err()
    );
    let result = classify(&make(&[(2, 0), (0, 1)]), &rules, Some(tmp.path()))
        .err()
        .unwrap();
    assert!(result.to_string().contains("runtime_entry"), "{result:#}");
    assert!(
        !result.to_string().contains("foreign_export"),
        "only actual entry roots should be named: {result:#}"
    );
}

#[test]
fn js_dispatcher_keeps_its_own_native_validation_leaves() {
    let mut g = graph(
        &[
            "js_dispatcher",
            "perry_runtime::exception::throw_leaf::throw_core",
        ],
        &[(0, 1)],
    );
    g.nodes[0].indirect_calls = 1;
    let rules =
        crate::load_rules("js exact js_dispatcher -- the indirect edge invokes program code\n")
            .unwrap();
    let result = classify(&g, &rules, None).unwrap();
    assert_eq!(result.table["js_dispatcher"].throw_class, ThrowClass::Core);
    let mut g = graph(&["js_dispatcher"], &[]);
    g.nodes[0].indirect_calls = 1;
    let result = classify(&g, &rules, None).unwrap();
    assert_eq!(result.table["js_dispatcher"].throw_class, ThrowClass::None);
}

#[test]
fn an_interpreted_program_body_is_a_whole_body_cut() {
    let g = graph(
        &[
            "program_body",
            "perry_runtime::exception::throw_leaf::throw_core",
            "perry_runtime::exception::throw_leaf::throw_open",
            "perry_runtime::exception::throw_leaf::throw_program_value",
        ],
        &[(0, 1), (0, 2), (0, 3)],
    );
    let rules=crate::load_rules("program exact program_body -- interpreted program execution is counted separately\nthrow prefix perry_runtime::exception::throw_leaf:: -- test native transport leaves\n").unwrap();
    let result = classify(&g, &rules, None).unwrap();
    assert_eq!(result.table["program_body"].throw_class, ThrowClass::None);
}

#[test]
fn a_local_body_behind_only_a_program_call_edge_is_not_a_coverage_root() {
    let mut g = graph(
        &[
            "runtime_entry",
            "js_dispatcher",
            "local_body",
            "perry_runtime::exception::throw_leaf::throw_program_value",
        ],
        &[(0, 1), (2, 3)],
    );
    g.by_global.remove("local_body");
    g.nodes[1].indirect_calls = 1;
    let rules=crate::load_rules("js exact js_dispatcher -- indirect edge enters program code, including local function values\nthrow prefix perry_runtime::exception::throw_leaf:: -- test native transport leaves\n").unwrap();
    let result = classify(&g, &rules, None).unwrap();
    assert_eq!(result.table["runtime_entry"].throw_class, ThrowClass::None);
}
