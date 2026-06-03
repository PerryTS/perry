//! Property-based tests for perry-container-compose.
//!
//! Uses the `proptest` crate to verify correctness properties
//! across serialization, dependency resolution, YAML parsing,
//! env interpolation, and type validation.

use indexmap::IndexMap;
use perry_container_compose::backend::{CliProtocol, DockerProtocol};
use perry_container_compose::compose::resolve_startup_order;
use perry_container_compose::error::compose_error_to_js;
use perry_container_compose::error::ComposeError;
use perry_container_compose::types::{
    ComposeService, ComposeSpec, ContainerSpec, DependsOnCondition, DependsOnSpec, VolumeType,
};
use perry_container_compose::yaml::interpolate;
use proptest::prelude::*;
use std::collections::HashMap;

// ============ Arbitrary Strategies ============

/// Generate a valid image reference string.
fn arb_image() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_-]{1,15}(:[a-z0-9._-]+)?"
}

/// Generate a valid service name.
fn arb_service_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_-]{1,10}"
}

/// Generate an arbitrary ComposeSpec with 1–10 services.
fn arb_compose_spec() -> impl Strategy<Value = ComposeSpec> {
    proptest::collection::vec(
        (arb_service_name(), arb_image()).prop_map(|(name, image)| {
            let mut svc = ComposeService::default();
            svc.image = Some(image);
            (name, svc)
        }),
        1..=10,
    )
    .prop_map(|services_vec| {
        let mut services = IndexMap::new();
        for (name, svc) in services_vec {
            services.insert(name, svc);
        }
        ComposeSpec {
            services,
            ..Default::default()
        }
    })
}

/// Generate a ComposeSpec with a valid (acyclic) depends_on DAG.
fn arb_compose_spec_with_dag() -> impl Strategy<Value = ComposeSpec> {
    proptest::collection::vec(
        (
            arb_service_name(),
            proptest::collection::vec(arb_service_name(), 0..=3),
        )
            .prop_map(|(name, deps)| {
                let mut svc = ComposeService::default();
                svc.image = Some(format!("{}:latest", name));
                (name, deps)
            }),
        2..=8,
    )
    .prop_map(|items| {
        // Build a valid DAG: only allow deps on services that appear
        // earlier in the list (forward references only).
        let mut services = IndexMap::new();
        let existing_names: Vec<String> = items.iter().map(|(n, _)| n.clone()).collect();

        for (name, dep_names) in &items {
            let mut svc = ComposeService::default();
            svc.image = Some(format!("{}:latest", name));

            // Only keep deps that point to earlier services (guarantees no cycles)
            let valid_deps: Vec<String> = dep_names
                .iter()
                .filter(|dep| {
                    existing_names
                        .iter()
                        .position(|n| n == name)
                        .map(|my_idx| {
                            existing_names
                                .iter()
                                .position(|n| n == *dep)
                                .map(|dep_idx| dep_idx < my_idx)
                                .unwrap_or(false)
                        })
                        .unwrap_or(false)
                })
                .cloned()
                .collect();

            if !valid_deps.is_empty() {
                svc.depends_on = Some(DependsOnSpec::List(valid_deps));
            }
            services.insert(name.clone(), svc);
        }

        ComposeSpec {
            services,
            ..Default::default()
        }
    })
}

/// Generate a ComposeSpec with at least one dependency cycle.
fn arb_compose_spec_with_cycle() -> impl Strategy<Value = ComposeSpec> {
    // Strategy A: 2-node cycle using proptest::array
    let two_node = proptest::array::uniform2(
        proptest::string::string_regex("[a-z]{2,4}a").unwrap(),
    )
    .prop_map(|names| {
        let (a, b) = (names[0].clone(), names[1].clone());
        let mut services = IndexMap::new();

        let mut svc_a = ComposeService::default();
        svc_a.image = Some(format!("{}:latest", a));
        svc_a.depends_on = Some(DependsOnSpec::List(vec![b.clone()]));
        services.insert(a.clone(), svc_a);

        let mut svc_b = ComposeService::default();
        svc_b.image = Some(format!("{}:latest", b));
        svc_b.depends_on = Some(DependsOnSpec::List(vec![a]));
        services.insert(b, svc_b);

        services
    });

    // Strategy B: 3-node cycle using proptest::array
    let three_node =
        proptest::array::uniform3(proptest::string::string_regex("[a-z]{2,4}[xyz]").unwrap())
            .prop_map(|names| {
                let (x, y, z) = (names[0].clone(), names[1].clone(), names[2].clone());
                let mut services = IndexMap::new();

                let mut svc_x = ComposeService::default();
                svc_x.image = Some(format!("{}:latest", x));
                svc_x.depends_on = Some(DependsOnSpec::List(vec![z.clone()]));
                services.insert(x.clone(), svc_x);

                let mut svc_y = ComposeService::default();
                svc_y.image = Some(format!("{}:latest", y));
                svc_y.depends_on = Some(DependsOnSpec::List(vec![x.clone()]));
                services.insert(y.clone(), svc_y);

                let mut svc_z = ComposeService::default();
                svc_z.image = Some(format!("{}:latest", z));
                svc_z.depends_on = Some(DependsOnSpec::List(vec![y]));
                services.insert(z, svc_z);

                services
            });

    proptest::prop_oneof![two_node, three_node].prop_map(|services| ComposeSpec {
        services,
        ..Default::default()
    })
}

/// Generate an arbitrary ContainerSpec.
fn arb_container_spec() -> impl Strategy<Value = ContainerSpec> {
    (
        arb_image(),
        proptest::option::of(arb_service_name()),
        proptest::option::of(proptest::collection::vec("[0-9]{2,5}:[0-9]{2,5}", 0..=3)),
        proptest::option::of(proptest::collection::vec("/[a-z]:/[a-z]", 0..=3)),
        proptest::bool::ANY,
    )
        .prop_map(|(image, name, ports, volumes, read_only)| ContainerSpec {
            image,
            name,
            ports,
            volumes,
            read_only: Some(read_only),
            ..Default::default()
        })
}

/// Generate environment variable name.
fn arb_env_name() -> impl Strategy<Value = String> {
    "[A-Z][A-Z0-9_]{1,8}"
}

/// Generate a template string containing ${VAR} and ${VAR:-default} patterns.
fn arb_env_template() -> impl Strategy<Value = (String, HashMap<String, String>)> {
    (arb_env_name(), arb_env_name(), "[a-z0-9_]{0,10}").prop_map(|(var1, var2, default)| {
        let mut env = HashMap::new();
        env.insert(var1.clone(), "value1".to_string());
        // var2 is intentionally missing from env to test defaults

        // Template: prefix_${VAR1}_mid_${VAR2:-default}_suffix
        // Both vars are referenced via ${} syntax so interpolation actually expands them
        let template = format!("prefix_${{{}}}_mid_${{{}:-{}}}_suffix", var1, var2, default);

        (template, env)
    })
}

// ============ Property 2: ContainerSpec CLI argument round-trip ============
// Feature: perry-container, Property 2: ContainerSpec CLI argument round-trip
// Validates: Requirements 12.5

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_container_spec_cli_round_trip(spec in arb_container_spec()) {
        let protocol = DockerProtocol;
        let args = protocol.run_args(&spec);

        // Manual verification of some fields since we don't have a full inverse parser yet
        if let Some(name) = &spec.name {
            prop_assert!(args.contains(&"--name".to_string()));
            prop_assert!(args.contains(name));
        }
        if spec.read_only.unwrap_or(false) {
            prop_assert!(args.contains(&"--read-only".to_string()));
        }
        prop_assert!(args.contains(&spec.image));
    }
}

// ============ Property 11: Error propagation preserves code and message ============
// Feature: perry-container, Property 11: Error propagation preserves code and message
// Validates: Requirements 2.6, 12.2

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_error_propagation(code in -100i32..500i32, message in ".*") {
        let err = ComposeError::BackendError { code, message: message.clone() };
        let js_json = compose_error_to_js(&err);
        let val: serde_json::Value = serde_json::from_str(&js_json).unwrap();

        prop_assert_eq!(val["code"].as_i64().unwrap() as i32, code);
        prop_assert_eq!(val["message"].as_str().unwrap().contains(&message), true);
    }
}

// ============ Property 1: ComposeSpec JSON round-trip ============
// Feature: perry-container, Property 1: ComposeSpec serialization round-trip
// Validates: Requirements 7.12, 10.13, 12.6

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_compose_spec_json_round_trip(spec in arb_compose_spec()) {
        let json = serde_json::to_string(&spec).unwrap();
        let deserialised: ComposeSpec = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deserialised).unwrap();
        prop_assert_eq!(json, json2);
    }
}

// ============ Property 3: Topological sort respects depends_on ============
// Feature: perry-container, Property 3: Topological sort respects depends_on
// Validates: Requirements 6.4

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_topological_sort_respects_deps(spec in arb_compose_spec_with_dag()) {
        let order = resolve_startup_order(&spec).unwrap();

        // Build position map
        let pos: HashMap<&str, usize> = order
            .iter()
            .enumerate()
            .map(|(i, s)| (s.as_str(), i))
            .collect();

        // For every service with depends_on, verify dependencies come first
        for (name, service) in &spec.services {
            if let Some(deps) = &service.depends_on {
                for dep in deps.service_names() {
                    if let (Some(&dep_pos), Some(&name_pos)) =
                        (pos.get(dep.as_str()), pos.get(name.as_str()))
                    {
                        prop_assert!(
                            dep_pos < name_pos,
                            "dep {} (pos {}) should come before {} (pos {})",
                            dep, dep_pos, name, name_pos
                        );
                    }
                }
            }
        }

        // All services must be in the output
        prop_assert_eq!(order.len(), spec.services.len());
    }
}

// ============ Property 4: Cycle detection is complete ============
// Feature: perry-container, Property 4: Cycle detection is complete
// Validates: Requirements 6.5

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_cycle_detection_completeness(spec in arb_compose_spec_with_cycle()) {
        let result = resolve_startup_order(&spec);
        prop_assert!(result.is_err(), "cycle should be detected");

        if let Err(ComposeError::DependencyCycle { services }) = result {
            // All services in the cycle should be listed
            prop_assert!(
                !services.is_empty(),
                "cycle must list at least one service"
            );
            // The listed services should be a subset of defined services
            for svc in &services {
                prop_assert!(
                    spec.services.contains_key(svc),
                    "cycle service {} should be defined in spec",
                    svc
                );
            }
        } else {
            panic!("expected DependencyCycle error");
        }
    }
}

// ============ Property 5: YAML round-trip ============
// Feature: perry-container, Property 5: YAML round-trip preserves ComposeSpec
// Validates: Requirements 7.1, 7.2–7.7

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_yaml_round_trip(spec in arb_compose_spec()) {
        let yaml = serde_yaml::to_string(&spec).unwrap();
        let reparsed: ComposeSpec = ComposeSpec::parse_str(&yaml).unwrap();

        // Service names preserved
        prop_assert_eq!(
            reparsed.services.keys().collect::<Vec<_>>(),
            spec.services.keys().collect::<Vec<_>>()
        );

        // Image references preserved
        for (name, svc) in &spec.services {
            let reparsed_svc = &reparsed.services[name];
            prop_assert_eq!(
                reparsed_svc.image.as_deref(),
                svc.image.as_deref(),
                "image mismatch for service {}",
                name
            );
        }
    }
}

// ============ Property 6: Environment variable interpolation ============
// Feature: perry-container, Property 6: Environment variable interpolation correctness
// Validates: Requirements 7.8

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_env_interpolation((template, env) in arb_env_template()) {
        let result = interpolate(&template, &env);

        // No ${...} should remain unexpanded
        prop_assert!(
            !result.contains("${"),
            "template should be fully expanded, got: {}",
            result
        );

        // The result should start with "prefix_value1_mid_"
        prop_assert!(
            result.starts_with("prefix_value1_mid_"),
            "expected expanded var1, got prefix: {}",
            &result[..result.len().min(20)]
        );
        // The result should end with "_suffix"
        prop_assert!(
            result.ends_with("_suffix"),
            "expected _suffix ending, got: {}",
            result
        );
    }
}

// ============ Property 7: Compose file merge last-writer-wins ============
// Feature: perry-container, Property 7: Compose file merge is last-writer-wins
// Validates: Requirements 7.10, 9.2

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_merge_last_writer_wins(
        common_svc in arb_service_name(),
        only_a_svc in arb_service_name(),
        img_a in arb_image(),
        img_b in arb_image(),
    ) {
        // Ensure distinct names
        prop_assume!(common_svc != only_a_svc);
        prop_assume!(img_a != img_b);

        let mut spec_a = ComposeSpec::default();
        let mut svc_a_common = ComposeService::default();
        svc_a_common.image = Some(img_a.clone());
        spec_a.services.insert(common_svc.clone(), svc_a_common);

        let mut svc_a_only = ComposeService::default();
        svc_a_only.image = Some(format!("onlya-{}", &common_svc));
        spec_a.services.insert(only_a_svc.clone(), svc_a_only);

        let mut spec_b = ComposeSpec::default();
        let mut svc_b_common = ComposeService::default();
        svc_b_common.image = Some(img_b.clone());
        spec_b.services.insert(common_svc.clone(), svc_b_common);

        // Merge: B wins for common service
        spec_a.merge(spec_b);

        // Common service should have B's image
        prop_assert_eq!(
            spec_a.services[&common_svc].image.as_deref(),
            Some(img_b.as_str()),
            "common service should have B's image (last-writer-wins)"
        );

        // Only-A service should still be present
        prop_assert!(
            spec_a.services.contains_key(&only_a_svc),
            "service only in A should be preserved"
        );
    }
}

// ============ Property 8: DependsOnCondition rejects invalid values ============
// Feature: perry-container, Property 8: DependsOnCondition rejects invalid values
// Validates: Requirements 7.14

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_depends_on_condition_rejects_invalid(invalid in "[a-z]{3,20}") {
        // Valid values: "service_started", "service_healthy", "service_completed_successfully"
        let valid_values = [
            "service_started",
            "service_healthy",
            "service_completed_successfully",
        ];
        prop_assume!(!valid_values.contains(&invalid.as_str()));

        let yaml = format!("\"{}\"", invalid);
        let result = serde_yaml::from_str::<DependsOnCondition>(&yaml);
        prop_assert!(
            result.is_err(),
            "DependsOnCondition should reject invalid value '{}', got: {:?}",
            invalid,
            result
        );
    }
}

// ============ Property 9: VolumeType rejects invalid values ============
// Feature: perry-container, Property 9: VolumeType rejects invalid values
// Validates: Requirements 10.14

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn prop_volume_type_rejects_invalid(invalid in "[a-z]{3,20}") {
        // Valid values: "bind", "volume", "tmpfs", "cluster", "npipe", "image"
        let valid_values = ["bind", "volume", "tmpfs", "cluster", "npipe", "image"];
        prop_assume!(!valid_values.contains(&invalid.as_str()));

        let yaml = format!("\"{}\"", invalid);
        let result = serde_yaml::from_str::<VolumeType>(&yaml);
        prop_assert!(
            result.is_err(),
            "VolumeType should reject invalid value '{}', got: {:?}",
            invalid,
            result
        );
    }
}
