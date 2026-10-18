//! Builtin argument roles must preserve known callback and receiver flows.
use super::*;

fn no_top(pf: &ProgramFacts, key: &str) {
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!sites.is_empty(), "missing .{key}: {}", pf.dump_text());
    for s in sites {
        assert!(!s.top, ".{key} carries TOP: {}", pf.dump_text());
    }
}

#[test]
fn primitive_absent_methods_do_not_invoke_callbacks() {
    let pf = facts(
        r#"
        class P { x = 1; }
        function cb(p: any) { return p.x; }
        const s: any = "x";
        s.forEach(cb); s.isEmpty(cb);
        cb(new P());
    "#,
    );
    no_top(&pf, "x");
}

#[test]
fn primitive_ignored_arguments_do_not_escape_functions() {
    let pf = facts(
        r#"
        class P { x = 1; }
        function cb(p: any) { return p.x; }
        const s: any = "x";
        s.trim(cb); s.charAt(0, cb); s.slice(0, 1, cb);
        cb(new P());
    "#,
    );
    no_top(&pf, "x");
}

#[test]
fn conversion_does_not_call_a_function_body() {
    let pf = facts(
        r#"
        class P { x = 1; }
        function cb(p: any) { return p.x; }
        const s: any = "abc";
        s.slice(cb); Math.abs(cb);
        cb(new P());
    "#,
    );
    no_top(&pf, "x");
}

#[test]
fn conversion_runs_user_conversion_hooks() {
    let pf = facts(
        r#"
        class P { x = 1; }
        class Index { valueOf() { const p = new P(); p.x = "s" as any; return 0; } }
        const s: any = "abc";
        s.slice(new Index());
        console.log(new P().x);
    "#,
    );
    assert!(
        field(tmpl(&pf, "P"), "x").kind.desc().contains("str"),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn primitive_prototype_replacements_keep_top() {
    let pf = facts(
        r#"
        class P { x = 1; }
        function cb(p: any) { return p.x; }
        (String.prototype as any).forEach = function (f: any) { f({x: "s"}); };
        const s: any = "x"; s.forEach(cb); cb(new P());
    "#,
    );
    assert!(site(&pf, "x").top, "{}", pf.dump_text());
}

#[test]
fn array_callbacks_receive_elements_and_this_arg() {
    for method in ["forEach", "map", "filter", "some", "every", "find"] {
        let pf = facts(&format!(
            r#"
            class P {{ x = 1; }}
            class C {{ y = 2; }}
            const xs: any = [new P()];
            xs.{method}(function(this: any, p: any, i: any, a: any) {{ return p.x + this.y + a[0].x + i; }}, new C());
        "#
        ));
        no_top(&pf, "x");
        no_top(&pf, "y");
    }
}

#[test]
fn array_reduce_and_sort_keep_callback_arguments_known() {
    let pf = facts(
        r#"
        class P { x = 1; }
        const xs: any = [new P()];
        xs.reduce((acc: any, p: any) => { console.log(acc.x, p.x); return p; }, new P());
        xs.sort((a: any, b: any) => a.x - b.x);
    "#,
    );
    no_top(&pf, "x");
}

#[test]
fn array_from_maps_source_with_this_arg() {
    let pf = facts(
        r#"
        class P { x = 1; }
        class C { y = 2; }
        const out = Array.from([new P()], function(this: any, p: any) { console.log(this.y); return p; }, new C());
        console.log(out[0].x);
    "#,
    );
    no_top(&pf, "x");
    no_top(&pf, "y");
}

#[test]
fn json_callbacks_are_called_without_escaping() {
    let pf = facts(
        r#"
        class P { x = 1; }
        function replace(key: any, value: any) { const p = new P(); return p; }
        JSON.stringify({a: new P()}, replace);
        const p = JSON.parse("{}", replace);
        console.log(p.x);
    "#,
    );
    no_top(&pf, "x");
}

#[test]
fn string_replace_does_not_escape_replacer() {
    let pf = facts(
        r#"
        class P { x = 1; }
        function replace(text: any, offset: any, input: any) { const p = new P(); return p.x + text; }
        const s: any = "abc";
        console.log(s.replace("a", replace));
    "#,
    );
    no_top(&pf, "x");
}

#[test]
fn custom_replace_protocol_receives_replacement() {
    let pf = facts(
        r#"
        class P { x = 1; }
        const search = { [Symbol.replace](text: any, replacement: any) { return replacement(new P()); } };
        const s: any = "abc";
        s.replace(search, function(p: any) { return p.x; });
    "#,
    );
    assert!(!pf.stats.aborted, "{}", pf.dump_text());
    let x = site(&pf, "x");
    assert!(!x.receivers.is_empty(), "{}", pf.dump_text());
}

#[test]
fn read_only_predicates_and_boolean_do_not_call_function_values() {
    let pf = facts(
        r#"
        class P { x = 1; }
        function cb(p: any) { return p.x; }
        Number.isNaN(cb); Number.isInteger(cb); Boolean(cb); Math.random(cb);
        cb(new P());
    "#,
    );
    no_top(&pf, "x");
}

#[test]
fn array_prototype_replacement_keeps_top() {
    let pf = facts(
        r#"
        class P { x = 1; }
        (Array.prototype as any).forEach = function (f: any) { f({ x: "s" }); };
        const xs: any = [new P()];
        xs.forEach(function(p: any) { return p.x; });
    "#,
    );
    assert!(site(&pf, "x").top, "{}", pf.dump_text());
}

#[test]
fn static_builtin_aliases_retain_argument_roles_and_return_flow() {
    let pf = facts(
        r#"
        class P { x = 1; }
        const values = Object.values;
        const from = Array.from;
        const xs = from(values({p: new P()}));
        console.log(xs[0].x);
        const predicate = Number.isInteger;
        function cb(p: any) { return p.x; }
        predicate(cb); cb(new P());
    "#,
    );
    no_top(&pf, "x");
}

#[test]
fn static_builtin_alias_replacement_keeps_top() {
    let pf = facts(
        r#"
        class P { x = 1; }
        (Object as any).values = function (obj: any) { return [obj.p]; };
        const values = Object.values;
        const p = new P();
        values({p}); console.log(p.x);
    "#,
    );
    assert!(tmpl(&pf, "P").escape.is_some(), "{}", pf.dump_text());
}

#[test]
fn opaque_prototype_write_revokes_callback_summary() {
    let pf = facts(
        r#"
        class P { x = 1; }
        declare const proto: any;
        proto.forEach = function(f: any) { f({x: "s"}); };
        const xs: any = [new P()];
        xs.forEach(function(p: any) { return p.x; });
    "#,
    );
    assert!(site(&pf, "x").top, "{}", pf.dump_text());
}

#[test]
fn cyclic_container_conversion_reaches_a_fixed_point() {
    let pf = facts_with(
        r#"
        const xs: any[] = [];
        xs.push(xs);
        const ys: any[] = [xs];
        xs.push(ys);
        console.log(String(xs), ys.join());
    "#,
        FactsConfig {
            max_work: Some(20_000),
            ..Default::default()
        },
    );
    assert!(!pf.stats.aborted, "{}", pf.dump_text());
}

#[test]
fn conversion_object_results_are_only_inspected_not_escaped() {
    let mut src = String::new();
    for i in 0..40 {
        src.push_str(&format!("class C{i} {{ x=1; }} class B{i} {{ toString() {{ return new C{i}(); }} }} try {{ String(new B{i}()); }} catch {{}} console.log(new C{i}().x);"));
    }
    let pf = facts_with(
        &src,
        FactsConfig {
            widen_k: Some(16),
            ..Default::default()
        },
    );
    for i in 0..40 {
        assert!(
            tmpl(&pf, &format!("C{i}")).escape.is_none(),
            "{}",
            pf.dump_text()
        );
    }
}

#[test]
fn own_value_enumeration_excludes_inherited_methods() {
    let pf = facts(
        r#"
        class P { x = 1; }
        class C { inherited(p: any) { return p.x; } }
        Object.values(new C()).forEach((f: any) => f(new P()));
    "#,
    );
    assert!(site(&pf, "x").receivers.is_empty(), "{}", pf.dump_text());
    assert!(!site(&pf, "x").top, "{}", pf.dump_text());
}

#[test]
fn own_value_enumeration_follows_late_getters() {
    let pf = facts(
        r#"
        class P { x = 1; }
        const o: any = {};
        const values = Object.values(o);
        Object.defineProperty(o, "p", {get: function() { return new P(); }});
        values.forEach((p: any) => p.x);
    "#,
    );
    assert!(
        site(&pf, "x").receivers.iter().any(|r| r.ends_with(":P")),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn json_does_not_enumerate_function_properties() {
    let pf = facts(
        r#"
        class P { x = 1; }
        function f() {}
        (f as any).nested = {m: (p: any) => p.x};
        JSON.stringify(f, (key: any, value: any) => {
            if (value && value.m) value.m(new P());
            return value;
        });
    "#,
    );
    assert!(site(&pf, "x").receivers.is_empty(), "{}", pf.dump_text());
    assert!(!site(&pf, "x").top, "{}", pf.dump_text());
}

#[test]
fn absent_array_methods_do_not_invoke_callbacks() {
    let pf = facts(
        r#"
        class P { x = 1; }
        function cb(p: any) { return p.x; }
        const xs: any = [new P()];
        xs.isEmpty(cb); xs.missing(cb); cb(new P());
    "#,
    );
    no_top(&pf, "x");
}

#[test]
fn written_missing_array_method_keeps_top() {
    let pf = facts(
        r#"
        class P { x = 1; }
        (Array.prototype as any).missing = (cb: any) => cb({x: "s"});
        const xs: any = [new P()];
        xs.missing((p: any) => p.x);
    "#,
    );
    assert!(site(&pf, "x").top, "{}", pf.dump_text());
}
