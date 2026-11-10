//! The solver already treats an addon's exports and calls as unknown code.
//! These are controls for the separate prototype-census admission, and pass
//! without that admission as well.
use perry_wpfacts::{FactsConfig, ProgramFacts, Verdict};

fn facts(body: &str) -> ProgramFacts {
    // The resolver's authenticated addon edge uses this exact HIR operation;
    // the logical id's spelling and the source's types grant no facts.
    let src = format!(
        r#"
        function loadAddon() {{
            const record = {{ exports: {{}} }};
            process.dlopen(record, "arbitrary/logical-id");
            return record.exports;
        }}
        const addon = loadAddon();
        {body}
    "#
    );
    let ast = perry_parser::parse_typescript(&src, "native-addon.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "native-addon", "native-addon.ts").unwrap();
    let pf = perry_wpfacts::compute(
        &[("native-addon.ts".into(), &hir)],
        &FactsConfig {
            max_work: Some(50_000_000),
            ..Default::default()
        },
    );
    assert!(!pf.stats.aborted, "{}", pf.dump_text());
    pf
}

fn verdict(pf: &ProgramFacts, key: &str, proven: bool) {
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .collect();
    assert_eq!(sites.len(), 1, "{key}: {}", pf.dump_text());
    assert_eq!(
        !matches!(sites[0].verdict, Verdict::No(_)),
        proven,
        "{key}: {:?}\n{}",
        sites[0].verdict,
        pf.dump_text()
    );
}

#[test]
fn shadowing_store_site_is_unknown() {
    let pf = facts(
        r#"
        const base = { inherited: "base" };
        const child = Object.create(base);
        addon.setNamed(child, "inherited", "own");
        console.log(child.inherited);
    "#,
    );
    verdict(&pf, "inherited", false);
}

#[test]
fn own_field_site_after_addon_handoff_is_unknown() {
    let pf = facts(
        r#"
        const object = { ownValue: 1 };
        addon.setNamed(object, "ownValue", 2);
        console.log(object.ownValue);
    "#,
    );
    verdict(&pf, "ownValue", false);
}

#[test]
fn addon_callback_parameter_and_receiver_sites_are_unknown() {
    let pf = facts(
        r#"
        function callback(argument) { return argument.callbackArg + this.callbackThis; }
        callback.call({ callbackThis: 1 }, { callbackArg: 2 });
        addon.keep(callback);
    "#,
    );
    verdict(&pf, "callbackArg", false);
    verdict(&pf, "callbackThis", false);
}

#[test]
fn a_stored_addon_export_is_still_an_unknown_callee() {
    let pf = facts(
        r#"
        const write = addon.setNamed;
        const object = { storedValue: 1 };
        write(object, "storedValue", 2);
        console.log(object.storedValue);
    "#,
    );
    verdict(&pf, "storedValue", false);
}

#[test]
fn addon_call_results_are_unknown() {
    let pf = facts("console.log(addon.make().resultValue);");
    verdict(&pf, "resultValue", false);
}

#[test]
fn an_object_never_given_to_the_addon_stays_proven() {
    let pf = facts(
        r#"
        const privateObject = { controlValue: 1 };
        addon.setNamed({}, "other", 2);
        console.log(privateObject.controlValue);
    "#,
    );
    verdict(&pf, "controlValue", true);
}

#[test]
fn removing_the_addon_call_keeps_the_own_field_proven() {
    let pf = facts("const object = { ownValue: 1 }; console.log(object.ownValue);");
    verdict(&pf, "ownValue", true);
}
