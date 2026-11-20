//! The one-pass propagation against a plain per-item search.
use super::{merge_into, Graph};
use std::collections::{BTreeSet, VecDeque};

/// Deterministic pseudo-random graphs with cycles, self-loops and isolated
/// nodes.
fn graph(seed: u64, n: u32, m: usize) -> Vec<(u32, u32)> {
    let mut x = seed;
    let mut next = move || {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (x >> 33) as u32
    };
    (0..m).map(|_| (next() % n, next() % n)).collect()
}

fn search(n: u32, edges: &[(u32, u32)], seeds: &[(u32, u32)], node: u32) -> Vec<u32> {
    let mut out: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n as usize];
    let mut work: VecDeque<(u32, u32)> = seeds.iter().copied().collect();
    while let Some((v, item)) = work.pop_front() {
        if !out[v as usize].insert(item) {
            continue;
        }
        for (from, to) in edges {
            if *from == v {
                work.push_back((*to, item));
            }
        }
    }
    out[node as usize].iter().copied().collect()
}

#[test]
fn propagation_matches_a_per_item_search() {
    for seed in 0..40u64 {
        let n = 1 + (seed as u32 * 7) % 60;
        let edges = graph(seed, n, (seed as usize * 13) % 150);
        let seeds: Vec<(u32, u32)> = graph(seed ^ 0x9e37, n, 1 + seed as usize % 20)
            .into_iter()
            .map(|(node, item)| (node, item % 25))
            .collect();
        let g = Graph::new(n as usize, edges.iter().copied());
        let queries: Vec<u32> = (0..n).collect();
        let reached = g.propagate(&seeds, &queries);
        for node in 0..n {
            assert_eq!(
                reached.at(node),
                search(n, &edges, &seeds, node).as_slice(),
                "seed {seed} node {node}"
            );
        }
    }
}

#[test]
fn only_queried_nodes_keep_their_sets() {
    let edges = [(0, 1), (1, 2), (2, 1), (2, 3)];
    let g = Graph::new(4, edges.iter().copied());
    let reached = g.propagate(&[(0, 5), (2, 1)], &[3]);
    assert_eq!(reached.at(3), &[1, 5]);
    assert!(reached.at(0).is_empty());
}

#[test]
fn a_component_is_numbered_after_the_components_it_reaches() {
    for seed in 0..20u64 {
        let edges = graph(seed, 30, 70);
        let g = Graph::new(30, edges.iter().copied());
        let (component, _) = g.components();
        for (from, to) in edges {
            assert!(component[to as usize] <= component[from as usize]);
        }
    }
}

#[test]
fn merge_keeps_one_sorted_copy() {
    let mut into = vec![1, 4, 9];
    merge_into(&mut into, &[0, 4, 5, 10]);
    assert_eq!(into, [0, 1, 4, 5, 9, 10]);
    let mut empty = Vec::new();
    merge_into(&mut empty, &[2, 3]);
    assert_eq!(empty, [2, 3]);
}

fn binding(id: u32, init: perry_hir::Expr) -> perry_hir::Stmt {
    perry_hir::Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: perry_hir::types::Type::Any,
        mutable: false,
        init: Some(init),
    }
}

fn property(object: perry_hir::Expr, key: &str) -> perry_hir::Expr {
    perry_hir::Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(object),
        property: key.into(),
        byte_offset: 0,
    }
}

fn store(object: perry_hir::Expr, key: &str, value: perry_hir::Expr) -> perry_hir::Stmt {
    perry_hir::Stmt::Expr(perry_hir::Expr::PropertySet {
        object: Box::new(object),
        property: key.into(),
        value: Box::new(value),
        byte_offset: 0,
        byte_end: 0,
    })
}

#[test]
fn equal_field_names_on_distinct_receivers_do_not_union_birth_slots() {
    use perry_hir::{Expr, Module, Stmt};
    let mut hir = Module::new("distinct_field_holders");
    hir.init = vec![
        binding(1, Expr::Array(vec![], None)),
        binding(2, Expr::Array(vec![], None)),
        binding(3, Expr::Object(vec![("items".into(), Expr::LocalGet(1))])),
        binding(4, Expr::Object(vec![])),
        store(Expr::LocalGet(4), "items", Expr::LocalGet(2)),
        store(
            property(Expr::LocalGet(3), "items"),
            "left",
            Expr::Integer(1),
        ),
        store(
            property(Expr::LocalGet(4), "items"),
            "right",
            Expr::Integer(2),
        ),
    ];
    super::assign_birth_layouts(&mut hir);
    for (stmt, expected) in hir.init[..2].iter().zip(["left", "right"]) {
        let Stmt::Let {
            init: Some(Expr::Array(_, Some(layout))),
            ..
        } = stmt
        else {
            panic!("the directly identified holder lost its array layout: {stmt:?}");
        };
        assert_eq!(layout.keys, [expected]);
    }
}
