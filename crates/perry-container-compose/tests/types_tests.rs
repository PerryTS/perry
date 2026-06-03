use perry_container_compose::types::*;
use proptest::prelude::*;
use serde_json;

// Feature: perry-container | Layer: unit | Req: 10.11 | Property: -
#[test]
fn test_list_or_dict_to_map() {
    let dict = ListOrDict::Dict({
        let mut m = indexmap::IndexMap::new();
        m.insert("KEY".into(), Some(serde_yaml::Value::String("VAL".into())));
        m
    });
    let map = dict.to_map();
    assert_eq!(map.get("KEY").unwrap(), "VAL");

    let list = ListOrDict::List(vec!["KEY=VAL".into()]);
    let map = list.to_map();
    assert_eq!(map.get("KEY").unwrap(), "VAL");
}

prop_compose! {
    fn arb_service_name()(s in "[a-z0-9_-]{1,10}") -> String { s }
}

prop_compose! {
    fn arb_image_ref()(s in "[a-z0-9._/-]{1,20}") -> String { s }
}

prop_compose! {
    fn arb_port_spec()(s in "[0-9]{1,5}:[0-9]{1,5}") -> PortSpec { PortSpec::Short(serde_yaml::Value::String(s)) }
}

prop_compose! {
    fn arb_list_or_dict()(m in prop::collection::hash_map("[A-Z]{1,5}", "[a-z]{1,5}", 0..5)) -> ListOrDict {
        let mut im = indexmap::IndexMap::new();
        for (k, v) in m {
            im.insert(k, Some(serde_yaml::Value::String(v)));
        }
        ListOrDict::Dict(im)
    }
}

prop_compose! {
    fn arb_depends_on_spec()(names in prop::collection::vec(arb_service_name(), 0..3)) -> DependsOnSpec {
        DependsOnSpec::List(names)
    }
}

prop_compose! {
    fn arb_compose_service()(
        image in prop::option::weighted(0.9, arb_image_ref()),
        ports in prop::option::weighted(0.5, prop::collection::vec(arb_port_spec(), 0..2)),
        environment in prop::option::weighted(0.5, arb_list_or_dict()),
        depends_on in prop::option::weighted(0.5, arb_depends_on_spec()),
    ) -> ComposeService {
        ComposeService {
            image,
            ports,
            environment,
            depends_on,
            ..Default::default()
        }
    }
}

prop_compose! {
    fn arb_compose_spec()(
        services in prop::collection::hash_map(arb_service_name(), arb_compose_service(), 1..5)
    ) -> ComposeSpec {
        let mut im = indexmap::IndexMap::new();
        for (k, v) in services {
            im.insert(k, v);
        }
        ComposeSpec {
            services: im,
            ..Default::default()
        }
    }
}

const PROPTEST_CASES: u32 = 256;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(PROPTEST_CASES))]

    // Feature: perry-container | Layer: property | Req: 12.6 | Property: 1
    #[test]
    fn prop_compose_spec_json_round_trip(spec in arb_compose_spec()) {
        let json = serde_json::to_string(&spec).unwrap();
        let de: ComposeSpec = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&de).unwrap();
        assert_eq!(json, json2);
    }
}

// Coverage Table:
// | Requirement | Test name | Layer |
// |-------------|-----------|-------|
// | 10.11       | test_list_or_dict_to_map | unit |
// | 12.6        | prop_compose_spec_json_round_trip | property |
