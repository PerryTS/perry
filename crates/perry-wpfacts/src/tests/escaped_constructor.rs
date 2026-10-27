//! CJL3: a function that escaped may be constructed by unknown code (`new f`
//! there), as an escaped class may (`escape_inner`): an instance of its
//! layout is born where nothing models it and reaches unknown code. Per unit
//! a unit that only writes `f.prototype` holds the layout with no instance of
//! its own (the units constructing it hold `f` as an unknown value): its copy
//! must still say what the whole program says of the instances, never a clean
//! layout (fastify `new Reply`, `new Request`, light-my-request
//! `new Response`, which escape through `Module._cache` in a program that is
//! not closed).

use super::*;

pub(super) fn fn_inst<'a>(pf: &'a ProgramFacts, name: &str) -> Vec<&'a crate::TemplateFacts> {
    pf.templates
        .iter()
        .filter(|t| t.kind == "fninst" && t.display.ends_with(&format!(":{name}")))
        .collect()
}

#[test]
fn an_escaped_constructor_function_has_instances_unknown_code_made() {
    // Nothing here constructs `F`; unknown code holds it.
    let pf = facts(
        r#"
        function F(x) { this.x = x; }
        F.prototype.get = function () { return this.x; };
        process.on("exit", F);
    "#,
    );
    let ts = fn_inst(&pf, "F");
    assert!(!ts.is_empty(), "{}", pf.dump_text());
    for t in ts {
        assert!(t.escape.is_some() && !t.prefix_stable, "{}", pf.dump_text());
    }
}

#[test]
fn a_constructor_function_nothing_lets_escape_keeps_its_layout() {
    // The control: constructed here only, it keeps its facts.
    let pf = facts(
        r#"
        function F(x) { this.x = x; }
        F.prototype.get = function () { return this.x; };
        const f = new F(1);
        console.log(f.get(), f.x);
    "#,
    );
    let ts = fn_inst(&pf, "F");
    assert!(!ts.is_empty(), "{}", pf.dump_text());
    for t in ts {
        assert!(t.escape.is_none() && t.prefix_stable, "{}", pf.dump_text());
    }
}
