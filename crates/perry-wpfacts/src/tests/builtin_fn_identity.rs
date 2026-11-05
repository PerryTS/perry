//! One runtime function object has one whole-program identity.
use super::*;

fn verdict(src: &str, marker: &str, key: &str, proven: bool) {
    let end = src.find(marker).expect("named site");
    let start = src[..end].rfind('\n').map_or(0, |n| n + 1);
    let pf = facts(src);
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| {
            !s.is_call && s.key == key && (start as u32..end as u32).contains(&s.byte_offset)
        })
        .collect();
    assert!(!sites.is_empty(), "missing {marker}/{key}");
    for s in sites {
        assert_eq!(
            !matches!(s.verdict, Verdict::No(_)),
            proven,
            "{marker}/{key}: {:?}\n{src}\n{}",
            s.verdict,
            pf.dump_text()
        );
    }
}

fn effect(f: &str, alias: &str, change: &str, call: &str, proven: bool) {
    let src = format!(
        r#"
declare function external(x: any): void;
const o: any = {{ field: 1 }};
function g(p: any) {{ p.field = 2; external(p); }}
const f: any = {f};
const alias: any = {alias};
{change}
{call}
console.log(o.field); // OBSERVE
"#
    );
    verdict(&src, "// OBSERVE", "field", proven);
}

fn prototypes(f: &str, alias: &str, proven: bool) {
    for change in [
        "alias.__proto__ = { zap: g };",
        "Object.setPrototypeOf(alias, { zap: g });",
    ] {
        for call in ["f.zap(o);", "const r = f.zap; r(o);"] {
            effect(f, alias, change, call, proven);
        }
    }
}

#[test]
fn set_keys_and_values_share_replaced_prototype_member_and_plain_calls() {
    prototypes("new Set().keys", "new Set().values", false);
}

#[test]
fn prototype_and_instance_has_own_property_are_one_function() {
    prototypes(
        "Object.prototype.hasOwnProperty",
        "({}).hasOwnProperty",
        false,
    );
    prototypes(
        "({}).hasOwnProperty",
        "Object.prototype.hasOwnProperty",
        false,
    );
}

#[test]
fn anonymous_native_replacement_reaches_each_identified_function() {
    for f in [
        "Object.prototype.hasOwnProperty",
        "Array.prototype.map",
        "new Set().keys",
    ] {
        prototypes(f, "[1][process.argv[2] ?? 'map']", false);
    }
}

#[test]
fn alias_getter_returns_the_same_function_for_member_and_plain_calls() {
    for call in ["f.zap(o);", "const r = f.zap; r(o);"] {
        effect(
            "new Set().keys",
            "new Set().values",
            "Object.defineProperty(alias, 'zap', { get() { return g; } });",
            call,
            false,
        );
        effect(
            "Object.prototype.hasOwnProperty",
            "({}).hasOwnProperty",
            "Object.defineProperty(alias, 'zap', { get() { return g; } });",
            call,
            false,
        );
    }
}

#[test]
fn alias_exposure_makes_the_identified_missing_member_unknown() {
    effect(
        "new Set().keys",
        "new Set().values",
        "external(alias);",
        "f.zap(o);",
        false,
    );
    effect(
        "Object.prototype.hasOwnProperty",
        "({}).hasOwnProperty",
        "external(alias);",
        "f.zap(o);",
        false,
    );
}

#[test]
fn null_alias_prototype_does_not_prove_an_inherited_function_member() {
    let src = "const f:any=new Set().keys; const alias:any=new Set().values;\n\
        Object.setPrototypeOf(alias, null);\n\
        console.log(f.call); // INHERITED\n";
    verdict(src, "// INHERITED", "call", false);
}

#[test]
fn array_and_map_iterator_aliases_share_prototype_mutations() {
    prototypes("[].values", "[][Symbol.iterator]", false);
    prototypes("new Map().entries", "new Map()[Symbol.iterator]", false);
    prototypes("new Set().keys", "new Set()[Symbol.iterator]", false);
}

#[test]
fn legacy_string_aliases_share_prototype_mutations() {
    prototypes("''.trimLeft", "''.trimStart", false);
    prototypes("''.trimRight", "''.trimEnd", false);
}

#[test]
fn date_gmt_and_utc_aliases_share_prototype_mutations() {
    prototypes("new Date().toGMTString", "new Date().toUTCString", false);
}

#[test]
fn different_set_function_keeps_observation_proven() {
    prototypes("new Set().keys", "new Set().add", true);
}

#[test]
fn different_map_function_keeps_observation_proven() {
    prototypes("new Map().entries", "new Map().get", true);
}

#[test]
fn keys_from_two_set_instances_are_the_same_prototype_member() {
    prototypes("new Set().keys", "new Set().keys", false);
}

#[test]
fn identified_has_own_property_still_converts_the_program_key() {
    let src = r#"
        declare function external(x: any): void;
        const o: any = { field: 1 };
        const f: any = new Map().hasOwnProperty;
        const key: any = { toString() { external(o); return "k"; } };
        f.call({}, key);
        console.log(o.field); // CONVERT
    "#;
    verdict(src, "// CONVERT", "field", false);
}

#[test]
fn anonymous_native_getter_and_exposure_reach_identified_values() {
    for f in [
        "Object.prototype.hasOwnProperty",
        "Array.prototype.map",
        "new Set().keys",
    ] {
        effect(
            f,
            "[1][process.argv[2] ?? 'map']",
            "Object.defineProperty(alias, 'zap', { get() { return g; } });",
            "f.zap(o);",
            false,
        );
        effect(
            f,
            "[1][process.argv[2] ?? 'map']",
            "external(alias);",
            "f.zap(o);",
            false,
        );
    }
}

#[test]
fn anonymous_prototype_reads_see_replacements_of_identified_values() {
    let src = r#"
        declare function external(x:any):void;
        const o:any={field:1};
        const f:any=Array.prototype.map;
        Object.setPrototypeOf(f, { zap() { external(o); } });
        const anonymous:any=[1][process.argv[2] ?? 'map'];
        const p:any=Object.getPrototypeOf(anonymous);
        p.zap();
        console.log(o.field); // PROTOTYPE-READ
    "#;
    verdict(src, "// PROTOTYPE-READ", "field", false);
}
