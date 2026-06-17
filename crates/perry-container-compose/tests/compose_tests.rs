use indexmap::IndexMap;
use perry_container_compose::compose::resolve_startup_order;
use perry_container_compose::types::{ComposeService, ComposeSpec, DependsOnSpec};
use proptest::prelude::*;

// Feature: perry-container | Layer: unit | Req: 6.4 | Property: 3
#[test]
fn test_resolve_startup_order_linear() {
    let mut services = IndexMap::new();
    services.insert("a".into(), ComposeService::default());
    services.insert(
        "b".into(),
        ComposeService {
            depends_on: Some(DependsOnSpec::List(vec!["a".into()])),
            ..Default::default()
        },
    );

    let spec = ComposeSpec {
        services,
        ..Default::default()
    };
    let order = resolve_startup_order(&spec).expect("should resolve");
    assert_eq!(order, vec!["a", "b"]);
}

// Feature: perry-container | Layer: unit | Req: 6.5 | Property: 4
#[test]
fn test_resolve_startup_order_cycle() {
    let mut services = IndexMap::new();
    services.insert(
        "a".into(),
        ComposeService {
            depends_on: Some(DependsOnSpec::List(vec!["b".into()])),
            ..Default::default()
        },
    );
    services.insert(
        "b".into(),
        ComposeService {
            depends_on: Some(DependsOnSpec::List(vec!["a".into()])),
            ..Default::default()
        },
    );

    let spec = ComposeSpec {
        services,
        ..Default::default()
    };
    let err = resolve_startup_order(&spec).unwrap_err();
    match err {
        perry_container_compose::error::ComposeError::DependencyCycle { services } => {
            assert!(services.contains(&"a".into()));
            assert!(services.contains(&"b".into()));
        }
        _ => panic!("Expected DependencyCycle error"),
    }
}

// Feature: perry-container | Layer: unit | Req: 6.4 | Property: 3
#[test]
fn test_resolve_startup_order_missing_dep() {
    let mut services = IndexMap::new();
    services.insert(
        "a".into(),
        ComposeService {
            depends_on: Some(DependsOnSpec::List(vec!["missing".into()])),
            ..Default::default()
        },
    );

    let spec = ComposeSpec {
        services,
        ..Default::default()
    };
    let err = resolve_startup_order(&spec).unwrap_err();
    assert!(err.to_string().contains("not defined"));
}

// Feature: perry-container | Layer: unit | Req: 6.4 | Property: 3
#[test]
fn test_resolve_startup_order_deterministic() {
    let mut services = IndexMap::new();
    services.insert("b".into(), ComposeService::default());
    services.insert("a".into(), ComposeService::default());

    let spec = ComposeSpec {
        services,
        ..Default::default()
    };
    let order = resolve_startup_order(&spec).expect("should resolve");
    assert_eq!(order, vec!["a", "b"]);
}

// Property-based tests

prop_compose! {
    fn arb_service_name()(s in "[a-z0-9_-]{1,10}") -> String { s }
}

prop_compose! {
    fn arb_compose_spec_dag(max_services: usize)(
        names in prop::collection::vec(arb_service_name(), 1..max_services).prop_map(|v| {
            let mut seen = std::collections::HashSet::new();
            v.into_iter().filter(|n| seen.insert(n.clone())).collect::<Vec<_>>()
        })
    )(
        names in Just(names.clone()),
        edges in {
            let mut strategies = Vec::new();
            for i in 0..names.len() {
                if i == 0 {
                    strategies.push(Just(vec![]).boxed());
                } else {
                    strategies.push(prop::collection::vec(0..i, 0..i.min(2)).boxed());
                }
            }
            strategies
        }
    ) -> ComposeSpec {
        let mut services = IndexMap::new();
        let names_list: Vec<String> = names;
        for (i, name) in names_list.iter().enumerate() {
            let mut svc = ComposeService::default();
            let svc_edges: &Vec<usize> = &edges[i];
            if !svc_edges.is_empty() {
                svc.depends_on = Some(DependsOnSpec::List(
                    svc_edges.iter().map(|&idx| names_list[idx].clone()).collect()
                ));
            }
            services.insert(name.clone(), svc);
        }
        ComposeSpec { services, ..Default::default() }
    }
}

const PROPTEST_CASES: u32 = 256;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(PROPTEST_CASES))]

    // Feature: perry-container | Layer: property | Req: 6.4 | Property: 3
    #[test]
    fn prop_topological_sort_respects_deps(spec in arb_compose_spec_dag(10)) {
        let order = resolve_startup_order(&spec).unwrap();
        let pos: std::collections::HashMap<_, _> = order.iter().enumerate().map(|(i, s)| (s, i)).collect();

        for (name, svc) in &spec.services {
            if let Some(deps) = &svc.depends_on {
                for dep in deps.service_names() {
                    assert!(pos[name] > pos[&dep], "Service {} must start after dependency {}", name, dep);
                }
            }
        }
    }
}

// Coverage Table:
// | Requirement | Test name | Layer |
// |-------------|-----------|-------|
// | 6.4         | test_resolve_startup_order_linear | unit |
// | 6.4         | test_resolve_startup_order_missing_dep | unit |
// | 6.4         | test_resolve_startup_order_deterministic | unit |
// | 6.4         | prop_topological_sort_respects_deps | property |
// | 6.5         | test_resolve_startup_order_cycle | unit |
