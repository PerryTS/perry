//! Resume arguments follow the transformed step ABI, independently of names.
//! Controls: next-only one-parameter steps, abrupt two-parameter steps, direct
//! and delegate call/apply wrappers, spread arguments, and numeric/string fields.
use super::*;

fn transformed(src: &str) -> ProgramFacts {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

fn delegate(call: &str) -> ProgramFacts {
    transformed(&format!(
        r#"
        class Returned {{ value: any; constructor(v: any) {{ this.value = v; }} }}
        class Numeric {{ value = 1; }}
        class Text {{ value = "control"; }}
        const custom = {{
            [Symbol.iterator]() {{ return this; }},
            next() {{ return {{ value: 1, done: false }}; }},
            return(v: any) {{ return new Returned(v); }}
        }};
        function* outer() {{ yield* custom; }}
        const a = outer(); a.next();
        {call}
        console.log(new Numeric().value, new Text().value);
    "#
    ))
}

#[test]
fn delegated_return_receives_string_and_number_resume_values() {
    let pf = delegate("a.return(\"early\"); a.return(77);");
    let k = &field(tmpl(&pf, "Returned"), "value").kind;
    assert!(!k.top && k.mask() & 3 == 3, "{}", pf.dump_text());
    assert_eq!(field(tmpl(&pf, "Numeric"), "value").kind.mask(), 1);
    assert_eq!(field(tmpl(&pf, "Text"), "value").kind.mask(), 2);
}

#[test]
fn spread_resume_value_reaches_the_step_argument() {
    let pf = delegate("a.return(...[\"spread\"]);");
    let k = &field(tmpl(&pf, "Returned"), "value").kind;
    assert!(!k.top && k.mask() & 2 != 0, "{}", pf.dump_text());
}

#[test]
fn next_only_step_keeps_its_single_argument_abi() {
    let pf = transformed(
        r#"
        class Resumed { value: any; constructor(v: any) { this.value = v; } }
        function* single() { const value = yield 1; return new Resumed(value); }
        const g = single(); g.next(); g.next("next");
    "#,
    );
    let k = &field(tmpl(&pf, "Resumed"), "value").kind;
    assert!(!k.top && k.mask() & 2 != 0, "{}", pf.dump_text());
}

#[test]
fn delegate_call_and_apply_wrappers_keep_the_resume_value() {
    let pf = transformed(
        r#"
        class Returned { value: any; constructor(v: any) { this.value = v; } }
        function forward(v: any) { return new Returned(v); }
        const custom = {
            [Symbol.iterator]() { return this; },
            next() { return { value: 1, done: false }; },
            return(v: any) { return forward.call(this, v); },
            throw(v: any) { return forward.apply(this, [v]); }
        };
        function* outer() { yield* custom; }
        const a = outer(); a.next(); a.return("call"); a.throw(77);
    "#,
    );
    let k = &field(tmpl(&pf, "Returned"), "value").kind;
    assert!(!k.top && k.mask() & 3 == 3, "{}", pf.dump_text());
}

#[test]
fn next_and_throw_resume_values_reach_two_parameter_steps() {
    let pf = transformed(
        r#"
        class NextValue { value: any; constructor(v: any) { this.value = v; } }
        class Thrown { value: any; constructor(v: any) { this.value = v; } }
        const custom = {
            [Symbol.iterator]() { return this; },
            next(v: any) { return new NextValue(v); },
            throw(v: any) { return new Thrown(v); }
        };
        function* outer() { yield* custom; }
        const a = outer(); a.next(); a.next("resume"); a.throw(77);
    "#,
    );
    let next = &field(tmpl(&pf, "NextValue"), "value").kind;
    let thrown = &field(tmpl(&pf, "Thrown"), "value").kind;
    assert!(!next.top && next.mask() & 2 != 0, "{}", pf.dump_text());
    assert!(!thrown.top && thrown.mask() & 1 != 0, "{}", pf.dump_text());
}
