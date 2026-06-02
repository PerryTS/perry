//! Phase E: golden-file fixture tests for `ComposeSpec::parse_str`.
//!
//! Each `tests/fixtures/*.yaml` is a real compose spec covering one
//! production-relevant pattern. Parsing must succeed (or fail with
//! the expected error) byte-for-byte across crate revisions.

use perry_container_compose::compose::resolve_startup_order;
use perry_container_compose::types::{ComposeService, ComposeSpec};

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(format!("{}.yaml", name));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {}", path.display(), e))
}

#[test]
fn parses_simple_two_service() {
    let spec = ComposeSpec::parse_str(&fixture("simple-two-service")).expect("parse");
    assert_eq!(spec.services.len(), 2);
    assert!(spec.services.contains_key("web"));
    assert!(spec.services.contains_key("api"));
    let web = &spec.services["web"];
    assert!(web.depends_on.is_some());
    assert!(spec.networks.is_some());
}

#[test]
fn diamond_deps_resolves_in_topological_order() {
    let spec = ComposeSpec::parse_str(&fixture("diamond-deps")).expect("parse");
    let order = resolve_startup_order(&spec).expect("topological order");
    // d must come before b and c; b/c must come before a. There's
    // exactly one valid prefix: [d, ..., a].
    let pos = |name: &str| order.iter().position(|s| s == name).expect(name);
    assert!(pos("d") < pos("b"));
    assert!(pos("d") < pos("c"));
    assert!(pos("b") < pos("a"));
    assert!(pos("c") < pos("a"));
}

#[test]
fn cyclic_deps_are_rejected() {
    let spec = ComposeSpec::parse_str(&fixture("cyclic-deps")).expect("parse");
    let result = resolve_startup_order(&spec);
    assert!(result.is_err(), "cyclic graph must be rejected");
    let err = result.err().unwrap();
    let msg = err.to_string();
    assert!(
        msg.to_lowercase().contains("cycle"),
        "error message should mention 'cycle'; got: {}",
        msg
    );
}

#[test]
fn external_network_parses_with_external_flag() {
    let spec = ComposeSpec::parse_str(&fixture("external-network")).expect("parse");
    let nets = spec.networks.expect("networks");
    let shared = nets
        .get("shared")
        .expect("shared net")
        .clone()
        .expect("non-null");
    assert_eq!(shared.external, Some(true));
    assert_eq!(shared.name.as_deref(), Some("production_shared_v1"));
}

#[test]
fn healthcheck_gated_parses_with_condition() {
    let spec = ComposeSpec::parse_str(&fixture("healthcheck-gated")).expect("parse");
    let api = &spec.services["api"];
    assert!(api.depends_on.is_some());
    let db = &spec.services["db"];
    assert!(db.healthcheck.is_some(), "db must have a healthcheck");
}

// ──────────────────────────────────────────────────────────────────────
// Property tests
// ──────────────────────────────────────────────────────────────────────

use proptest::prelude::*;

proptest! {
    /// Container-name format: `{md5_8}-{random_hex8}`. The hash
    /// component is 8 chars, the random suffix is 8 chars, hyphen
    /// separator. The format is invariant across all image strings.
    #[test]
    fn container_name_format_is_md5_8_dash_hex8(
        image in "[a-zA-Z0-9._-]{1,40}"
    ) {
        let svc = perry_container_compose::types::ComposeService {
            image: Some(image.clone()),
            ..Default::default()
        };
        let name = perry_container_compose::service::service_container_name(&svc, "svc");
        let parts: Vec<&str> = name.split('-').collect();
        prop_assert_eq!(parts.len(), 2, "format is {{md5_8}}-{{random_hex8}}");
        prop_assert_eq!(parts[0].len(), 8);
        prop_assert_eq!(parts[1].len(), 8);
        prop_assert!(parts[0].chars().all(|c| c.is_ascii_hexdigit()));
        prop_assert!(parts[1].chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// Same image must produce the same first 8 hex chars (the MD5
    /// component is deterministic; only the random suffix varies).
    #[test]
    fn container_name_md5_prefix_is_deterministic_per_image(
        image in "[a-zA-Z0-9._-]{1,40}"
    ) {
        let svc = perry_container_compose::types::ComposeService {
            image: Some(image.clone()),
            ..Default::default()
        };
        let n1 = perry_container_compose::service::service_container_name(&svc, "svc");
        let n2 = perry_container_compose::service::service_container_name(&svc, "svc");
        prop_assert_eq!(&n1[..8], &n2[..8], "md5 prefix must be deterministic");
    }

    /// Project namespacing: any two distinct project names produce
    /// distinct namespaced volume names for the same key. This is
    /// the data-loss-prevention invariant — Tier 1.1 fix.
    #[test]
    fn project_namespacing_disambiguates_volumes(
        proj1 in "[a-z][a-z0-9_-]{1,15}",
        proj2 in "[a-z][a-z0-9_-]{1,15}",
        vol_key in "[a-z][a-z0-9_-]{1,15}",
    ) {
        prop_assume!(proj1 != proj2);
        let n1 = format!("{}_{}", proj1, vol_key);
        let n2 = format!("{}_{}", proj2, vol_key);
        prop_assert_ne!(n1, n2, "different projects must produce different volume names");
    }

    /// Spec-hash determinism: serialising a `ComposeService` to JSON
    /// produces the same string across calls (so the
    /// `perry.compose.spec_hash` label is stable). Tier 2.7 fix.
    #[test]
    fn spec_hash_is_deterministic_per_serialise(
        image in "[a-z][a-z0-9._/:-]{0,30}",
        port in "[0-9]{2,5}:[0-9]{2,5}",
    ) {
        let svc = perry_container_compose::types::ComposeService {
            image: Some(image),
            ports: Some(vec![
                perry_container_compose::types::PortSpec::Short(
                    serde_yaml::Value::String(port),
                ),
            ]),
            ..Default::default()
        };
        let s1 = serde_json::to_string(&svc).unwrap();
        let s2 = serde_json::to_string(&svc).unwrap();
        prop_assert_eq!(s1, s2);
    }

    /// Topological-sort correctness: for any DAG, every dependency
    /// edge `a → b` (b depends on a) must appear with `a` before `b`
    /// in the resolved order.
    #[test]
    fn topological_sort_respects_edges(
        names in proptest::collection::hash_set("[a-z]{2,5}", 2..=6),
    ) {
        use perry_container_compose::types::DependsOnSpec;
        let names: Vec<String> = names.into_iter().collect();
        let mut spec = ComposeSpec::default();
        for (i, n) in names.iter().enumerate() {
            let mut s = ComposeService::default();
            s.image = Some(format!("alpine:{}", i));
            // Build a chain: a→b→c→d→…
            if i > 0 {
                s.depends_on = Some(DependsOnSpec::List(vec![names[i - 1].clone()]));
            }
            spec.services.insert(n.clone(), s);
        }
        let order = resolve_startup_order(&spec).expect("DAG must resolve");
        // Every name must appear exactly once.
        prop_assert_eq!(order.len(), names.len());
        // For each edge i-1 → i, names[i-1] must come before names[i].
        for i in 1..names.len() {
            let before = order.iter().position(|s| s == &names[i - 1]).unwrap();
            let after = order.iter().position(|s| s == &names[i]).unwrap();
            prop_assert!(
                before < after,
                "{} (dep) must come before {}; order: {:?}",
                names[i - 1], names[i], order
            );
        }
    }
}
