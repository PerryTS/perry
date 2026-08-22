//! Regression coverage for #8587.
//!
//! Release archives build `perry-stdlib-static` with its default feature set.
//! That eventually enables `perry-stdlib/full`, which must remain
//! self-contained: the `external-http-*-pump` features declare C symbols that
//! are provided only when the compiler also selects `libperry_ext_http.a`.
//! Making either pump part of `full` breaks otherwise HTTP-free prebuilt
//! consumers such as a minimal Linux `perry/ui` application.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn local_feature_closure(
    features: &toml::Table,
    roots: impl IntoIterator<Item = String>,
) -> BTreeSet<String> {
    let mut pending: Vec<String> = roots.into_iter().collect();
    let mut enabled = BTreeSet::new();

    while let Some(feature) = pending.pop() {
        if !enabled.insert(feature.clone()) {
            continue;
        }

        let Some(members) = features.get(&feature).and_then(toml::Value::as_array) else {
            continue;
        };
        for member in members.iter().filter_map(toml::Value::as_str) {
            // Only bare names refer to another feature in this manifest.
            // `dep:name` and `crate/feature` entries leave this feature graph.
            if !member.starts_with("dep:") && !member.contains('/') {
                pending.push(member.trim_end_matches('?').to_owned());
            }
        }
    }

    enabled
}

#[test]
fn release_full_stdlib_does_not_enable_external_http_pumps() {
    let manifest_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../perry-stdlib/Cargo.toml");
    let source = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", manifest_path.display()));
    let manifest: toml::Value = toml::from_str(&source)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", manifest_path.display()));
    let features = manifest
        .get("features")
        .and_then(toml::Value::as_table)
        .expect("perry-stdlib must declare a [features] table");

    for pump in ["external-http-client-pump", "external-http-server-pump"] {
        assert!(
            features.contains_key(pump),
            "the regression guard must track the current external HTTP pump feature name: {pump}"
        );
    }

    let default = local_feature_closure(features, ["default".to_owned()]);
    for pump in ["external-http-client-pump", "external-http-server-pump"] {
        assert!(
            !default.contains(pump),
            "perry-stdlib's release/default feature graph must not enable `{pump}`: it creates \
             unresolved libperry_ext_http.a references in HTTP-free prebuilt links (#8587)"
        );
    }
}
