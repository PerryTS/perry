use super::*;

fn graph_function(id: FuncId, dependencies: &[FuncId]) -> Function {
    Function {
        id,
        name: format!("f{id}"),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Void,
        body: dependencies
            .iter()
            .map(|id| Stmt::Expr(Expr::FuncRef(*id)))
            .collect(),
        is_async: false,
        is_generator: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
        is_strict: false,
    }
}

#[test]
fn graph_budget_stops_long_chain_before_descent() {
    let functions: Vec<_> = (0..64)
        .map(|id| graph_function(id, &if id < 63 { vec![id + 1] } else { vec![] }))
        .collect();
    let by_id = functions.iter().map(|f| (f.id, f)).collect();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut out = Vec::new();
    assert!(!collect_function_graph(
        0,
        &by_id,
        &mut visiting,
        &mut visited,
        &mut out,
        &HashSet::from([0])
    ));
    assert!(
        visiting.len() + visited.len() <= 8,
        "discovered {} functions before rejection",
        visiting.len() + visited.len()
    );
    assert!(
        out.is_empty(),
        "over-budget chain must stop before unwinding a leaf"
    );
}

fn graph_module(edges: &[&[FuncId]]) -> Module {
    let mut module = Module::new("/src/graph.ts");
    module.functions = edges
        .iter()
        .enumerate()
        .map(|(id, deps)| graph_function(id as FuncId, deps))
        .collect();
    module.functions[0].is_exported = true;
    module.exported_functions.push(("root".to_string(), 0));
    module
}

#[test]
fn graph_budget_accepts_exactly_eight_nodes_in_dependency_order() {
    let module = graph_module(&[&[1], &[2], &[3], &[4], &[5], &[6], &[7], &[]]);
    let gathered = gather_cross_module_functions(&module);
    let ids: Vec<_> = gathered["root"].functions.iter().map(|f| f.id).collect();
    assert_eq!(ids, vec![7, 6, 5, 4, 3, 2, 1, 0]);
}

#[test]
fn graph_budget_rejects_ninth_node() {
    let module = graph_module(&[&[1], &[2], &[3], &[4], &[5], &[6], &[7], &[8], &[]]);
    assert!(gather_cross_module_functions(&module).is_empty());
}

#[test]
fn graph_budget_accepts_shared_dependency_at_capacity() {
    let module = graph_module(&[&[1, 2], &[3], &[7], &[4], &[5], &[6], &[7], &[]]);
    let gathered = gather_cross_module_functions(&module);
    let ids: Vec<_> = gathered["root"].functions.iter().map(|f| f.id).collect();
    assert_eq!(ids, vec![7, 6, 5, 4, 3, 1, 2, 0]);
}

#[test]
fn graph_budget_counts_completed_siblings_before_descent() {
    let module = graph_module(&[&[1, 2], &[3], &[], &[4], &[5], &[6], &[7], &[8], &[]]);
    let by_id = module.functions.iter().map(|f| (f.id, f)).collect();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut out = Vec::new();
    assert!(!collect_function_graph(
        0,
        &by_id,
        &mut visiting,
        &mut visited,
        &mut out,
        &HashSet::from([0])
    ));
    assert_eq!(visiting.len() + visited.len(), 8);
    assert!(
        !visited.contains(&2),
        "the ninth node must not be processed"
    );
    assert!(gather_cross_module_functions(&module).is_empty());
}

#[test]
fn graph_budget_preserves_cycle_and_self_recursion_rejection() {
    for module in [
        graph_module(&[&[0]]),
        graph_module(&[&[1], &[2], &[1]]),
        graph_module(&[&[1], &[2], &[3], &[4], &[5], &[6], &[7], &[1]]),
    ] {
        assert!(gather_cross_module_functions(&module).is_empty());
    }
}

#[test]
fn graph_budget_bounds_long_cycle_discovery() {
    let functions: Vec<_> = (0..64)
        .map(|id| graph_function(id, &[(id + 1) % 64]))
        .collect();
    let by_id = functions.iter().map(|f| (f.id, f)).collect();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut out = Vec::new();
    assert!(!collect_function_graph(
        0,
        &by_id,
        &mut visiting,
        &mut visited,
        &mut out,
        &HashSet::from([0])
    ));
    assert_eq!(visiting.len(), 8);
    assert!(visited.is_empty());
    assert!(out.is_empty());
}

#[test]
fn graph_budget_preserves_separately_exported_dependency_rejection() {
    let mut module = graph_module(&[&[1], &[]]);
    module.functions[1].is_exported = true;
    module.exported_functions.push(("sibling".to_string(), 1));
    let gathered = gather_cross_module_functions(&module);
    assert!(!gathered.contains_key("root"));
    assert_eq!(gathered["sibling"].functions.len(), 1);
}

#[test]
fn graph_budget_preserves_wide_graph_root_and_node_limit() {
    for count in [1usize, 8, 9] {
        let edges: Vec<Vec<FuncId>> = (0..count)
            .map(|id| {
                if id == 0 {
                    (1..count as FuncId).collect()
                } else {
                    vec![]
                }
            })
            .collect();
        let refs: Vec<&[FuncId]> = edges.iter().map(Vec::as_slice).collect();
        let module = graph_module(&refs);
        let gathered = gather_cross_module_functions(&module);
        if count <= 8 {
            let candidate = &gathered["root"];
            assert_eq!(candidate.functions.len(), count);
            assert_eq!(candidate.root_id, 0);
            let actual: Vec<_> = candidate.functions.iter().map(|f| f.id).collect();
            let expected: Vec<_> = (1..count as FuncId).chain(std::iter::once(0)).collect();
            assert_eq!(actual, expected);
        } else {
            assert!(gathered.is_empty());
        }
    }
}

#[test]
fn graph_budget_reuses_completed_node_at_full_capacity() {
    let module = graph_module(&[&[1], &[2], &[3], &[4], &[5], &[6], &[7], &[]]);
    let by_id = module.functions.iter().map(|f| (f.id, f)).collect();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut out = Vec::new();
    assert!(collect_function_graph(
        0,
        &by_id,
        &mut visiting,
        &mut visited,
        &mut out,
        &HashSet::from([0])
    ));
    assert!(visiting.is_empty());
    assert_eq!(visited.len(), 8);
    assert_eq!(out, vec![7, 6, 5, 4, 3, 2, 1, 0]);
    assert!(collect_function_graph(
        7,
        &by_id,
        &mut visiting,
        &mut visited,
        &mut out,
        &HashSet::from([0])
    ));
    assert_eq!(out, vec![7, 6, 5, 4, 3, 2, 1, 0]);
    assert_eq!(visited.len(), 8);
}
