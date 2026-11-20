//! ECMA-262 13.5.1.2 / 10.1.10 [[Delete]]: `delete o[k]` removes at most the
//! own property `k` of `o`; it stores no value. A computed delete on an
//! unknown object (⊤, which may be a built-in prototype) can make a key
//! absent, never present: the implicit `toJSON` lookup of `JSON.stringify`
//! on an array (SerializeJSONProperty, 25.5.2.2 step 2), which misses
//! on the intrinsic chain and is parked (`intrinsic_miss`), still finds no
//! callable, so nothing it serializes reaches unknown code. The lookup is
//! answered by what a computed store through ⊤ stored (a number:
//! `answer_stored`); a delete must not turn that answer opaque.
use super::*;

fn src(body: &str) -> String {
    format!(
        r#"
        class M {{ m = 2; go() {{ return this.m; }} }}
        const u: any = new Function(process.argv[2])();
        const k: any = process.argv[3];
        // A computed store of a number through ⊤: `toJSON` may be written
        // on a prototype the lock leaves open (`Array.prototype`), holding 1.
        u[process.argv[4]] = 1;
        const box: any = [];
        box.push(new M());
        console.log(JSON.stringify(box));
        {body}
        console.log(box[0].go());
        "#
    )
}

fn escapes(pf: &ProgramFacts, name: &str) -> bool {
    tmpl(pf, name).escape.is_some()
}

#[test]
fn computed_delete_on_unknown_adds_no_to_json() {
    let pf = facts(&src("delete u[k];"));
    assert!(!escapes(&pf, "M"), "M escaped\n{}", pf.dump_text());
}

/// Control: the base case without the delete.
#[test]
fn no_delete_nothing_escapes() {
    let pf = facts(&src(""));
    assert!(!escapes(&pf, "M"), "M escaped\n{}", pf.dump_text());
}

/// Control: a value that really reaches unknown code escapes.
#[test]
fn handed_to_unknown_code_escapes() {
    let pf = facts(&src("delete u[k]; u(box);"));
    assert!(escapes(&pf, "M"), "{}", pf.dump_text());
}

/// Control: a named delete of `toJSON` on ⊤ records an absent `toJSON`.
#[test]
fn named_delete_stays_closed() {
    let pf = facts(&src("delete u.toJSON;"));
    assert!(!escapes(&pf, "M"), "M escaped\n{}", pf.dump_text());
}
