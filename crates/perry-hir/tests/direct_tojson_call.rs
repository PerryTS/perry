use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lower_debug(src: &str) -> String {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "direct_tojson_call.ts", &mut cache)
        .expect("parse should succeed");
    let module = lower_module(&parsed.module, "test", "direct_tojson_call.ts")
        .expect("lowering should succeed");
    format!("{module:#?}")
}

#[test]
fn user_class_tojson_calls_stay_generic() {
    let debug = lower_debug(
        r#"
        class Fixture {
            value = "deploy";
            toJSON(): any {
                return { name: this.value };
            }
            describe(): any {
                return { label: this.value };
            }
        }

        const fixture: any = new Fixture();
        const out = {
            direct: fixture.toJSON(),
            directNew: new Fixture().toJSON(),
            bracket: fixture["toJSON"](),
            computed: fixture["to" + "JSON"](),
            callResult: fixture.toJSON.call(fixture),
            control: fixture.describe(),
        };
        "#,
    );

    assert!(
        !debug.contains("DateToJSON"),
        "userland toJSON calls must not lower to DateToJSON: {debug}"
    );
    assert!(
        debug.contains("property: \"toJSON\""),
        "direct toJSON calls should stay on the generic property-call path: {debug}"
    );
    assert!(
        debug.contains("property: \"describe\""),
        "ordinary method-name control should stay generic too: {debug}"
    );
}

#[test]
fn unknown_tojson_receiver_stays_generic() {
    let debug = lower_debug(
        r#"
        function makeBuilder(): any {
            return {};
        }

        const command: any = makeBuilder();
        const out = command.toJSON();
        "#,
    );

    assert!(
        !debug.contains("DateToJSON"),
        "unknown toJSON receivers must not lower to DateToJSON: {debug}"
    );
    assert!(
        debug.contains("property: \"toJSON\""),
        "unknown direct toJSON calls should stay generic: {debug}"
    );
}

#[test]
fn known_date_tojson_stays_date_intrinsic() {
    let debug = lower_debug(
        r#"
        const inferred = new Date(0);
        const annotated: Date = new Date(0);
        const out = {
            inline: new Date(0).toJSON(),
            inferred: inferred.toJSON(),
            annotated: annotated.toJSON(),
        };
        "#,
    );

    assert!(
        debug.contains("DateToJSON"),
        "known Date toJSON calls should still lower to DateToJSON: {debug}"
    );
}
