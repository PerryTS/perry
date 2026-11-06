use super::*;
use crate::model::{prim, TmplKind, TokKind};
use crate::{ElementFacts, ElementKey, ElementOp};

fn at<'a>(pf: &'a ProgramFacts, src: &str, text: &str, op: ElementOp) -> &'a ElementFacts {
    let offset = src.find(text).expect("site text") as u32 + 1;
    pf.element(0, offset, op)
        .unwrap_or_else(|| panic!("missing {op:?} at {offset}: {:#?}", pf.elements))
}

#[test]
fn string_argument_has_string_mask_without_top() {
    let src = r#"function get(recv: any, key: any) { return recv[key]; }
        console.log(get("abc", 0));"#;
    let pf = facts(src);
    let site = at(&pf, src, "recv[key]", ElementOp::Read);
    assert!(!site.top, "{site:?}");
    assert_eq!(site.prims, prim::STR);
    assert!(site.receivers.is_empty(), "{site:?}");
}

#[test]
fn array_argument_has_array_token_without_string() {
    let src = r#"function get(recv: any, key: any) { return recv[key]; }
        console.log(get([1, 2], 0));"#;
    let pf = facts(src);
    let site = at(&pf, src, "recv[key]", ElementOp::Read);
    assert!(!site.top, "{site:?}");
    assert_eq!(site.prims & prim::STR, 0);
    assert!(!site.receivers.is_empty());
    for value in &site.receivers {
        assert!(matches!(value.kind, TokKind::Obj(..)), "{value:?}");
        assert!(!value.marker);
        assert!(
            matches!(value.template_kind, Some(TmplKind::Arr { .. })),
            "{value:?}"
        );
    }
}

#[test]
fn computed_this_read_holds_program_class_instances() {
    let src = r#"class Box {
        value = 7;
        get(key: any) { return this[key]; }
    }
    console.log(new Box().get("value"));"#;
    let pf = facts(src);
    let site = at(&pf, src, "this[key]", ElementOp::Read);
    assert!(!site.top, "{site:?}");
    assert_eq!(site.prims, 0);
    let id = tmpl(&pf, "Box").id;
    assert!(!site.receivers.is_empty());
    assert!(
        site.receivers
            .iter()
            .all(|v| matches!(v.kind, TokKind::Obj(t, _) if t == id)),
        "{site:?}"
    );
}

#[test]
fn parsed_json_receiver_has_json_domain_and_all_primitives() {
    let src = r#"const recv: any = JSON.parse('{"value":7}');
        const key: any = "value"; console.log(recv[key]);"#;
    let pf = facts(src);
    let site = at(&pf, src, "recv[key]", ElementOp::Read);
    assert!(!site.top, "{site:?}");
    assert_eq!(site.prims, prim::ALL);
    assert!(!site.receivers.is_empty());
    assert!(
        site.receivers
            .iter()
            .all(|v| matches!(v.kind, TokKind::Obj(..))),
        "{site:?}"
    );
}

#[test]
fn unmodeled_global_receiver_is_explicit_top() {
    let src = r#"const recv: any = (globalThis as any).unknownThing;
        const key: any = "value"; console.log(recv[key]);"#;
    let pf = facts(src);
    let site = at(&pf, src, "recv[key]", ElementOp::Read);
    assert!(site.top, "{site:?}");
    assert!(site
        .receivers
        .iter()
        .any(|v| matches!(v.kind, TokKind::Top)));
}

#[test]
fn counting_loop_key_is_number_only() {
    let src = r#"const recv: any = [1, 2, 3];
        for (let i = 0; i < 3; i++) { console.log(recv[i]); }"#;
    let pf = facts(src);
    let site = at(&pf, src, "recv[i]", ElementOp::Read);
    assert_eq!(site.key, ElementKey::NumberOnly);
}

#[test]
fn computed_write_and_call_keep_distinct_operations() {
    let src = r#"const recv: any = { value: 1, method: () => 2 };
        const key: any = "value";
        recv[key] = 3;
        const method: any = "method"; console.log(recv[method]());"#;
    let pf = facts(src);
    let write = at(&pf, src, "recv[key] = 3", ElementOp::Write);
    assert!(!write.top);
    let call = at(&pf, src, "recv[method]", ElementOp::Call);
    assert_eq!(call.key, ElementKey::AnyString);
    assert!(pf.element(0, call.byte_offset, ElementOp::Read).is_none());
}

#[test]
fn named_and_element_sites_share_unguarded_receiver_values() {
    let src = r#"class Box { value = 7; }
        function get(recv: any, key: any) {
            console.log(recv.value); return recv[key];
        }
        console.log(get(new Box(), "value"));"#;
    let pf = facts(src);
    let indexed = at(&pf, src, "recv[key]", ElementOp::Read);
    let named = site(&pf, "value");
    assert_eq!(named.receiver_values, indexed.receivers);
    assert_eq!(named.receiver_prims, indexed.prims);
    assert_eq!(
        named.receivers,
        indexed
            .receivers
            .iter()
            .map(|v| v.display.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn string_key_variable_does_not_invent_a_constant_set() {
    let src = r#"const recv: any = { value: 7 };
        const key: any = "value"; console.log(recv[key]);"#;
    let pf = facts(src);
    assert_eq!(
        at(&pf, src, "recv[key]", ElementOp::Read).key,
        ElementKey::AnyString
    );
}

#[test]
fn an_aborted_solve_publishes_no_element_proofs() {
    let src = r#"const recv: any = [1, 2]; const key: any = 0; console.log(recv[key]);"#;
    assert!(!facts(src).elements.is_empty());
    let pf = facts_with(
        src,
        FactsConfig {
            max_work: Some(1),
            ..Default::default()
        },
    );
    assert!(pf.stats.aborted);
    assert!(pf.elements.is_empty());
    assert!(pf
        .element(
            0,
            src.find("recv[key]").unwrap() as u32 + 1,
            ElementOp::Read
        )
        .is_none());
}

#[test]
fn computed_update_has_both_operations_at_one_offset() {
    let src = "const recv: any = [1]; const key: any = 0; recv[key]++;";
    let pf = facts(src);
    let read = at(&pf, src, "recv[key]", ElementOp::Read);
    let write = at(&pf, src, "recv[key]", ElementOp::Write);
    assert_eq!(read.receivers, write.receivers);
    assert_eq!(read.prims, write.prims);
}

#[test]
fn named_putvalue_is_also_recorded() {
    let src = "const recv: any = { value: 1 }; recv.value = 2;";
    let pf = facts(src);
    let write = at(&pf, src, "recv.value = 2", ElementOp::Write);
    assert_eq!(write.key, ElementKey::Named("value".into()));
}

#[test]
fn optional_computed_read_keeps_its_source_identity() {
    let src = "const recv: any = [1]; const key: any = 0; console.log(recv?.[key]);";
    let pf = facts(src);
    let read = at(&pf, src, "recv?.[key]", ElementOp::Read);
    assert!(!read.top);
    assert!(read
        .receivers
        .iter()
        .any(|v| matches!(v.template_kind, Some(TmplKind::Arr { .. }))));
}

#[test]
fn wrapped_compound_access_keeps_both_source_operations() {
    let src = "function get(): any { return [1]; } const key: any = 0; get()[key] += 1;";
    let pf = facts(src);
    let read = at(&pf, src, "get()[key]", ElementOp::Read);
    let write = at(&pf, src, "get()[key]", ElementOp::Write);
    assert_eq!(read.receivers, write.receivers);
}

#[test]
fn builtin_alias_receivers_reuse_canonical_identity_and_shape() {
    let src = r#"const recv: any = new Set().keys;
        const alias: any = new Set().values;
        const key: any = "name";
        console.log(recv[key], alias[key], recv.name);"#;
    let pf = facts(src);
    let indexed = at(&pf, src, "recv[key]", ElementOp::Read);
    let alias = at(&pf, src, "alias[key]", ElementOp::Read);
    assert!(!indexed.top, "{indexed:?}");
    assert_eq!(indexed.prims, 0);
    assert_eq!(indexed.receivers.len(), 1, "{indexed:?}");
    let value = &indexed.receivers[0];
    assert!(
        matches!(
            value.kind,
            TokKind::BuiltinFn(crate::model::Native::Member { .. })
        ),
        "{value:?}"
    );
    assert!(value.shape.is_some(), "{value:?}");
    assert_eq!(indexed.receivers, alias.receivers);
    assert_eq!(site(&pf, "name").receiver_values, indexed.receivers);
}
