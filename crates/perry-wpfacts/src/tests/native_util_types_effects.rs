//! The `node:util/types` brand predicates declare their effects at the body
//! their dispatch arm calls (an effects-only `realm_decl!` above
//! `js_util_types_is_array_buffer_view` and its siblings): the call reads
//! no property of its argument, calls nothing, keeps nothing and returns a
//! boolean. Named control: `isGeneratorObject` reads fields of its argument
//! and stays undeclared, so its argument still escapes.
use super::*;

const FORMS: [(&str, &str); 3] = [
    ("import * as t from \"util/types\";", "t.isArrayBufferView"),
    (
        "import { isArrayBufferView } from \"node:util/types\";",
        "isArrayBufferView",
    ),
    ("const t = require(\"util/types\");", "t.isArrayBufferView"),
];

fn passed(import: &str, call: &str) -> crate::ProgramFacts {
    facts(&format!(
        r#"
        {import}
        class Q {{ y = 1; }}
        const o: any = {{ m(a: any) {{ return a.y; }} }};
        function f(v: any) {{ return v.q; }}
        console.log(f({call}(o)));
        console.log(o.m(new Q()));
    "#
    ))
}

#[test]
fn is_array_buffer_view_keeps_its_argument_and_returns_a_boolean() {
    for (import, call) in FORMS {
        let pf = passed(import, call);
        let y = site(&pf, "y");
        assert!(
            !y.top && y.receivers.iter().all(|r| r.ends_with(":Q")),
            "{import}\n{}",
            pf.dump_text()
        );
        let q = site(&pf, "q");
        assert!(
            !q.top && q.receivers.is_empty(),
            "{import}\n{}",
            pf.dump_text()
        );
    }
}

#[test]
fn every_declared_brand_predicate_returns_a_boolean() {
    for p in [
        "isDate",
        "isMap",
        "isTypedArray",
        "isProxy",
        "isNativeError",
    ] {
        let pf = passed("import * as t from \"util/types\";", &format!("t.{p}"));
        let q = site(&pf, "q");
        assert!(!q.top && q.receivers.is_empty(), "{p}\n{}", pf.dump_text());
    }
}

#[test]
fn is_generator_object_stays_undeclared() {
    let pf = passed("import * as t from \"util/types\";", "t.isGeneratorObject");
    let y = site(&pf, "y");
    assert!(y.top, "{}", pf.dump_text());
}
