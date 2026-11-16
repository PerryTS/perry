//! ECMA-262 23.2.3.31 %TypedArray%.prototype.toLocaleString with ECMA-402
//! 19.5.1: the method reads neither `locales` nor `options`; it Invokes
//! each element's `toLocaleString(locales, options)`. The elements are
//! Numbers or BigInts, whose toLocaleString builds a NumberFormat:
//! CanonicalizeLocaleList reads `locales` (length, elements, ToString) and
//! GetOption reads the NumberFormat keys of `options` (getters run there).
//! Nothing else of either operand is read, and no unknown code receives it.
use super::*;

fn src(body: &str) -> String {
    format!(
        r#"
        class K {{ k = 1; }}
        class M {{ m = 2; go() {{ return this.m; }} }}
        class H {{ h = 3; ping() {{ return this.h; }} }}
        const hh = new H();
        const loc: any = [new K()];
        const opts: any = {{ get style() {{ hh.ping(); return "decimal"; }}, extra: new M() }};
        {body}
        "#
    )
}

fn escapes(pf: &ProgramFacts, name: &str) -> bool {
    tmpl(pf, name).escape.is_some()
}

fn ping_reached(pf: &ProgramFacts) -> bool {
    pf.sites
        .iter()
        .any(|s| s.key == "ping" && s.is_call && s.verdict == Verdict::T0Mono)
}

#[test]
fn typed_to_locale_string_passes_no_operand_to_unknown_code() {
    let pf = facts(&src(
        "const ta = new Float32Array(2); console.log(ta.toLocaleString(loc, opts));",
    ));
    let d = pf.dump_text();
    assert!(!escapes(&pf, "M"), "M escaped\n{d}");
    assert!(!escapes(&pf, "K"), "K escaped\n{d}");
    assert!(ping_reached(&pf), "style getter not reached\n{d}");
}

#[test]
fn number_to_locale_string_reads_the_option_getters() {
    let pf = facts(&src("console.log((5).toLocaleString(loc, opts));"));
    let d = pf.dump_text();
    assert!(!escapes(&pf, "M"), "M escaped\n{d}");
    assert!(ping_reached(&pf), "style getter not reached\n{d}");
}

/// Control: options made by unknown code stay unknown: a value stored into
/// them escapes.
#[test]
fn unknown_options_stay_unknown() {
    let pf = facts(&src(
        "const u: any = new Function(process.argv[2])(); u.extra = new M();
         const ta = new Float32Array(2); console.log(ta.toLocaleString(loc, u));",
    ));
    assert!(escapes(&pf, "M"), "{}", pf.dump_text());
}

/// Control: Array.prototype.toLocaleString Invokes each element's own
/// method with the operands: a program method receives the options.
#[test]
fn array_element_method_receives_the_options() {
    let pf = facts(&src(
        "const xs: any = [{ toLocaleString(l: any, o: any) { return o.extra.go(); } }];
         console.log(xs.toLocaleString(loc, opts));",
    ));
    let reached = pf
        .sites
        .iter()
        .any(|s| s.key == "go" && s.is_call && s.verdict == Verdict::T0Mono);
    assert!(reached, "{}", pf.dump_text());
}
