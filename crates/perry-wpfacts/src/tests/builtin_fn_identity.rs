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

fn timer_source(prefix: &str, call: &str) -> String {
    format!(
        r#"
declare function external(x:any):void;
class Box {{ field = 1; }}
function cb(p:any=new Box()) {{
    console.log(p.field); // CALLBACK
}}
function g(f:any) {{ external(f); }}
{prefix}
{call}
"#
    )
}

#[test]
fn global_timer_value_uses_callback_defaults() {
    for call in ["setTimeout(cb, 0);", "const t=setTimeout; t(cb, 0);"] {
        verdict(&timer_source("", call), "// CALLBACK", "field", true);
    }
}

#[test]
fn timer_value_through_program_parameter_keeps_its_callback_model() {
    verdict(
        &timer_source("function run(t:any) { t(cb,0); }", "run(setTimeout);"),
        "// CALLBACK",
        "field",
        true,
    );
}

#[test]
fn namespace_value_call_and_extracted_call_retain_results() {
    for call in [
        "const result=Object.assign(o,{});",
        "const f=Object.assign; const result=f(o,{});",
        "const f:any=Object.assign; const result=f.call(null,o,{});",
    ] {
        let src =
            format!("const o:any={{field:1}};\n{call}\nconsole.log(result.field); // RESULT\n");
        verdict(&src, "// RESULT", "field", true);
    }
}

#[test]
fn namespace_callback_parameter_retains_the_known_result() {
    let src = r#"
        const o:any={field:1};
        function run(generateName:any, node:any) { return generateName(node,{}); }
        const result=run(Object.assign,o);
        console.log(result.field); // GENERATED
    "#;
    verdict(src, "// GENERATED", "field", true);
}

#[test]
fn number_and_global_parse_functions_share_fields_and_prototypes() {
    for name in ["parseFloat", "parseInt"] {
        let src=format!("const f:any=Number.{name}; const alias:any={name};\nf.cache={{field:1}};\nconsole.log(alias.cache.field); // PARSE\n");
        verdict(&src, "// PARSE", "field", true);
        prototypes(&format!("Number.{name}"), name, false);
    }
}

#[test]
fn named_function_prototype_members_keep_distinct_fields() {
    let src = r#"
        declare function unknown():any;
        const a:any=(()=>0).apply;
        const b:any=Function.prototype.call;
        a.cache=unknown();
        b.cache={field:1};
        console.log(b.cache.field); // FUNCTION-MEMBER
    "#;
    verdict(src, "// FUNCTION-MEMBER", "field", true);
}

#[test]
fn replaced_global_timer_value_stays_unproven() {
    verdict(
        &timer_source("globalThis.setTimeout=g;", "const t=setTimeout; t(cb,0);"),
        "// CALLBACK",
        "field",
        false,
    );
}

#[test]
fn unknown_global_key_store_keeps_timer_value_unproven() {
    verdict(
        &timer_source(
            "declare function key():any; (globalThis as any)[key()]=g;",
            "const t=setTimeout; t(cb,0);",
        ),
        "// CALLBACK",
        "field",
        false,
    );
}

#[test]
fn global_exposure_keeps_timer_value_unproven() {
    verdict(
        &timer_source("external(globalThis);", "const t=setTimeout; t(cb,0);"),
        "// CALLBACK",
        "field",
        false,
    );
}

#[test]
fn function_value_exposure_preserves_the_timer_body_model() {
    verdict(
        &timer_source("const t=setTimeout; external(t);", "t(cb,0);"),
        "// CALLBACK",
        "field",
        true,
    );
}

#[test]
fn module_scope_timer_binding_wins_over_the_global() {
    verdict(
        &timer_source("const setTimeout:any=g;", "const t=setTimeout; t(cb,0);"),
        "// CALLBACK",
        "field",
        false,
    );
}

#[test]
fn undeclared_global_function_value_stays_unproven() {
    verdict(
        // The timer is an unseen module's global.
        &unseen(&timer_source("", "const t=CXID_undeclared_timer; t(cb,0);")),
        "// CALLBACK",
        "field",
        false,
    );
}

#[test]
fn linked_dynamic_code_keeps_global_function_value_unproven() {
    let src = timer_source(
        "eval('globalThis.setTimeout = function (f) { f({}); }');",
        "const t=setTimeout; t(cb,0);",
    );
    let cfg = FactsConfig {
        dynamic_code: Some("test linked interpreter".into()),
        interpreted_sources: true,
        ..Default::default()
    };
    let pf = facts_with(&src, cfg);
    let marker = src.find("// CALLBACK").unwrap();
    let start = src[..marker].rfind('\n').unwrap() as u32;
    let site = pf
        .sites
        .iter()
        .find(|s| !s.is_call && s.key == "field" && (start..marker as u32).contains(&s.byte_offset))
        .unwrap();
    assert!(matches!(site.verdict, Verdict::No(_)), "{}", pf.dump_text());
}

// Compare the actual callback Call constraints, not the spelling of the
// dispatch implementation. The callback node is symbolic; the source-site
// checks below also require the callback body to receive the program value.
fn timer_callback_constraints(direct: bool) -> Vec<(Vec<u32>, Vec<u32>, Vec<String>)> {
    use crate::model::{BuiltinNs, CallSite, Cons, Key, Native, ThisArg, TokKind};
    let mut m = crate::gen::Gen::new(Vec::new(), false, false).m;
    m.max_work = 50_000_000;
    let cb = m.node_l("callback");
    let delay = m.node_l("delay");
    let extra = m.node_l("extra");
    let ret = m.node_l("return");
    m.calls.push(CallSite {
        this: ThisArg::Undef,
        args: vec![cb, delay, extra],
        spread: vec![],
        ret,
        site: 0,
        region: m.thrown,
    });
    let g = m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
    let recv = m.node_l("receiver");
    let key = m.atom("setTimeout");
    if direct {
        m.apply(
            recv,
            &Cons::MCall {
                key: Key::Named(key),
                call: 0,
            },
            &[g],
            0,
        );
    } else {
        let f = m.tok(TokKind::BuiltinFn(Native::Member {
            identity: "setTimeout",
            holder: None,
        }));
        m.apply(recv, &Cons::Call { call: 0 }, &[f], 0);
    }
    m.solve();
    m.nodes[cb as usize]
        .cons
        .iter()
        .filter_map(|c| {
            let Cons::Call { call } = c else {
                return None;
            };
            let c = &m.calls[*call as usize];
            let mut this = match c.this {
                ThisArg::Node(n) => m.nodes[n as usize]
                    .pts
                    .to_sorted()
                    .iter()
                    .map(|&t| m.tok_name(t))
                    .collect(),
                ThisArg::Tok(t) => vec![m.tok_name(t)],
                ThisArg::Undef => vec!["undefined".into()],
            };
            this.sort();
            Some((c.args.clone(), c.spread.clone(), this))
        })
        .collect()
}

#[test]
fn direct_and_value_timer_callback_constraints_are_identical() {
    for callee in ["setTimeout", "t"] {
        let src = format!(
            r#"
            class Box {{field=1;}}
            function cb(p:any) {{
                console.log(p.field); // SAME-CALLBACK
            }}
            const t=setTimeout;
            {callee}(cb,0,new Box());
        "#
        );
        verdict(&src, "// SAME-CALLBACK", "field", true);
    }
    let direct = timer_callback_constraints(true);
    let value = timer_callback_constraints(false);
    assert_eq!(direct.len(), 1, "{direct:?}");
    assert_eq!(
        direct, value,
        "callback argument / spread / this constraints differ"
    );
}

#[test]
fn anonymous_native_changes_reach_callable_namespace_values() {
    for f in ["Object", "Array", "setTimeout", "Math.abs"] {
        prototypes(f, "[1][process.argv[2] ?? 'map']", false);
    }
}

#[test]
fn null_alias_prototype_is_visible_to_the_prototype_value_read() {
    let src = r#"
        const f:any=new Set().keys;
        const alias:any=new Set().values;
        Object.setPrototypeOf(alias,null);
        const parent=Object.getPrototypeOf(f);
        console.log(parent.cxidProbe);
        console.log(f.call); // NULL-MEMBER
    "#;
    verdict(src, "// NULL-MEMBER", "call", false);
    let pf = facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    let lines = &pf.diagnostics[0].1;
    let number = |line: &str, key: &str| -> u32 {
        line.split(&format!("\"{key}\":"))
            .nth(1)
            .expect("field")
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect("number")
    };
    let site = lines
        .iter()
        .find(|l| l.contains("\"record\":\"site\"") && l.contains("\"key\":\"cxidProbe\""))
        .expect("prototype value site");
    let dst = number(site, "receiver");
    let node = lines
        .iter()
        .find(|l| l.contains(&format!("\"record\":\"node\",\"id\":{dst},")))
        .expect("prototype result node");
    assert_ne!(
        number(node, "primitiveMask") & crate::model::prim::NULL as u32,
        0,
        "{node}"
    );
}

#[test]
fn prototype_constructor_and_global_constructor_are_one_value() {
    let src = r#"
        const f:any=Set;
        const alias:any=Set.prototype.constructor;
        alias.cache={field:1};
        console.log(f.cache.field); // CONSTRUCTOR-ID
    "#;
    verdict(src, "// CONSTRUCTOR-ID", "field", true);
}

#[test]
fn restoring_the_declared_global_value_does_not_expose_its_members() {
    for (change, proven) in [
        ("(globalThis as any).Object=Object;", true),
        (
            "(globalThis as any).Object=Object; external(Object);",
            false,
        ),
    ] {
        let src = format!(
            r#"
            declare function external(x:any):void;
            const o:any={{field:1}};
            function load(){{external(o);}}
            const unknown:any=(globalThis as any)[process.argv[2]];
            {change}
            Object.keys(load as any);
            console.log(o.field); // RETAINED-GLOBAL
        "#
        );
        let ast = perry_parser::parse_typescript(&src, "t.ts").unwrap();
        let mut pb = perry_hir::patched_builtins::PatchedBuiltins::new();
        perry_hir::scan_patched_builtins(&ast, &mut pb);
        perry_hir::patched_builtins::set_patched_builtins(std::sync::Arc::new(pb));
        let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
        perry_hir::patched_builtins::clear_patched_builtins();
        let pf = crate::compute(&[("t.ts".into(), &hir)], &FactsConfig::default());
        let site = pf
            .sites
            .iter()
            .find(|s| s.key == "field" && !s.is_call)
            .expect("retained global observation");
        assert_eq!(
            !matches!(site.verdict, Verdict::No(_)),
            proven,
            "{change}\n{}",
            pf.dump_text()
        );
    }
}

#[test]
fn annexb_accessor_installed_on_a_global_or_namespace_value_keeps_identity() {
    for (member, alias) in [
        ("Number.parseInt", "parseInt"),
        ("setTimeout", "globalThis.setTimeout"),
        ("Object.assign", "Object.assign"),
    ] {
        for install in [false, true] {
            let src = format!(
                r#"
                declare function external(x:any):void;
                const o:any={{field:1}};
                function g(){{external(o);}}
                const f:any={member};
                const alias:any={alias};
                {}
                alias.zap(o);
                console.log(o.field); // ANNEX-VALUE
            "#,
                if install {
                    "f.__defineGetter__('zap',()=>g);"
                } else {
                    ""
                }
            );
            verdict(&src, "// ANNEX-VALUE", "field", !install);
        }
    }
}

#[test]
fn an_identified_unmodeled_constructor_keeps_its_declared_static_member() {
    let src = r#"
        const ctor:any=URL;
        const f:any=ctor.parse;
        const alias:any=URL.parse;
        f.cache={field:1};
        console.log(alias.cache.field); // CONSTRUCTOR-STATIC
    "#;
    verdict(src, "// CONSTRUCTOR-STATIC", "field", true);
}

#[test]
fn an_unmodeled_constructor_static_value_still_has_opaque_effects() {
    let src = r#"
        declare function external(x:any):void;
        const o:any={field:1,toString(){external(o);return 'https://example.org';}};
        const ctor:any=URL;
        ctor.canParse(o);
        console.log(o.field); // CONSTRUCTOR-EFFECT
    "#;
    verdict(src, "// CONSTRUCTOR-EFFECT", "field", false);
    let construct = src.replace("ctor.canParse(o)", "new ctor(o)");
    verdict(&construct, "// CONSTRUCTOR-EFFECT", "field", false);
}

#[test]
fn inherited_namespace_function_values_keep_distinct_identities() {
    for holder in ["Date", "RegExp", "ArrayBuffer", "URL"] {
        let src = format!(
            r#"
            const f:any={holder}.toString;
            const a:any={holder}.valueOf;
            f.cache={{field:1}};
            console.log(a.cache.field); // INHERITED-DIFFERENT
        "#
        );
        verdict(&src, "// INHERITED-DIFFERENT", "field", false);
    }
}

#[test]
fn inherited_typed_array_statics_have_one_identity_per_function() {
    for (member, proven) in [("Int8Array.from", true), ("Uint8Array.of", false)] {
        let src = format!(
            r#"
            const f:any=Uint8Array.from;
            const a:any={member};
            f.cache={{field:1}};
            console.log(a.cache.field); // INHERITED-STATIC
        "#
        );
        verdict(&src, "// INHERITED-STATIC", "field", proven);
    }
}

#[test]
fn exposure_keeps_the_native_body_and_opens_its_missing_members() {
    for exposed in ["t", "[1][process.argv[2]??'map']"] {
        verdict(
            &timer_source(
                &format!("const t=setTimeout; external({exposed});"),
                "t(cb,0);",
            ),
            "// CALLBACK",
            "field",
            true,
        );
        for call in ["t.zap(o);", "const r=t.zap; r(o);"] {
            let src = format!(
                r#"
                declare function external(x:any):void;
                const o:any={{field:1}};
                const t:any=setTimeout;
                external({exposed});
                {call}
                console.log(o.field); // EXPOSED-MEMBER
            "#
            );
            verdict(&src, "// EXPOSED-MEMBER", "field", false);
        }
    }
}

fn property_receiver_primitives(src: &str, key: &str) -> u16 {
    let pf = facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    let lines = &pf.diagnostics[0].1;
    let number = |line: &str, key: &str| -> u32 {
        line.split(&format!("\"{key}\":"))
            .nth(1)
            .expect("diagnostic field")
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect("number")
    };
    let site = lines
        .iter()
        .find(|l| {
            l.contains("\"record\":\"site\"")
                && l.contains("\"kind\":\"property\"")
                && l.contains(&format!("\"key\":\"{key}\""))
        })
        .expect("property site");
    let receiver = number(site, "receiver");
    let node = lines
        .iter()
        .find(|l| l.contains(&format!("\"record\":\"node\",\"id\":{receiver},")))
        .expect("receiver node");
    number(node, "primitiveMask") as u16
}

#[test]
fn independent_anonymous_values_keep_the_absent_field_case() {
    let src = r#"
        const f:any=[1][process.argv[2]??'map'];
        const a:any=[1][process.argv[3]??'slice'];
        f.cache={field:1};
        console.log(a.cache.field); // NULLABLE-ANONYMOUS
    "#;
    assert_ne!(
        property_receiver_primitives(src, "field") & crate::model::prim::UNDEF,
        0
    );
}

#[test]
fn the_named_date_pair_keeps_the_absent_field_case() {
    let src = r#"
        const f:any=Date.toString;
        const a:any=Date.valueOf;
        f.cache={field:1};
        console.log(a.cache.field); // NULLABLE-DATE
    "#;
    assert_ne!(
        property_receiver_primitives(src, "field") & crate::model::prim::UNDEF,
        0
    );
}

#[test]
fn reading_back_through_the_same_anonymous_variable_keeps_its_nullable_layout_proof() {
    let src = r#"
        const f:any=[1][process.argv[2]??'map'];
        f.cache={field:1};
        console.log(f.cache.field); // NULLABLE-SAME
    "#;
    assert_ne!(
        property_receiver_primitives(src, "field") & crate::model::prim::UNDEF,
        0
    );
    verdict(src, "// NULLABLE-SAME", "field", true);
}

#[test]
fn a_prototype_constructor_value_called_as_a_callback_retains_its_result() {
    let src = r#"
        const o:any={field:1};
        function run(make:any,x:any){return make(x);}
        const make:any=Array.prototype.constructor;
        const result:any=run(make,o);
        console.log(result[0].field); // ARRAY-CTOR-CALLBACK
    "#;
    verdict(src, "// ARRAY-CTOR-CALLBACK", "field", true);
}

#[test]
fn an_extracted_native_call_method_retains_the_string_replace_result() {
    let src = r#"
        const call:any=String.prototype.replace.call;
        const result:any=call.call(String.prototype.replace,'x','x','z');
        console.log(result.length); // STRING-REPLACE-CALL
    "#;
    verdict(src, "// STRING-REPLACE-CALL", "length", true);
}

#[test]
fn cxid3_unknown_code_can_replace_the_json_function_before_a_length_read() {
    let src = r#"
const ns = JSON;
new Function("ns", process.argv[2])(ns);
const value = ns.stringify(0).slice(1);
console.log(value.length); // DYNAMIC-LENGTH
"#;
    verdict(src, "// DYNAMIC-LENGTH", "length", false);
}

#[test]
fn cxid3_a_wildcard_native_read_charges_every_holder_it_inspects() {
    use crate::model::{prim, ChainAct, Key, Model, Native, TokKind};
    let mut m = Model::default();
    m.tok(TokKind::Top);
    for i in 0..2000 {
        m.tok(TokKind::ModNs(i));
    }
    let identities: std::collections::BTreeSet<_> =
        include_str!("../builtins/realm_declared_keys.tsv")
            .lines()
            .filter_map(|line| {
                let mut row = line.split('\t');
                if row.next()? != "native" {
                    return None;
                }
                row.next()?;
                row.next()?;
                row.next()
            })
            .collect();
    for identity in identities {
        m.tok(TokKind::BuiltinFn(Native::Member { identity, holder: None }));
    }
    let f = m.tok(TokKind::BuiltinFn(Native::Any));
    let dst = m.node_l("bounded-native-read");
    // With no stored indexed fields, the work is the holder walk itself.
    let key = m.atom(crate::model::ELEM);
    let before = m.work.get();
    m.max_work = before + 128;
    m.builtin_field_use(f, Key::Named(key), ChainAct::Load(dst));
    let charged = m.work.get() - before;
    assert!(
        charged >= 1000,
        "{} token values, over 1000 native holders examined, {charged} work charged",
        m.toks.len()
    );
    m.add_prim(dst, prim::NUM);
    m.solve();
    assert!(m.aborted, "the native holder walk must obey the work bound");
}
