//! A computed key held by a `const` binding with a string-literal
//! initializer names that key (`Gen::key_of`), as the literal does: the
//! read sees only that field.
use super::*;

fn verdict(src: &str) -> Verdict {
    let pf = facts(src);
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "witness").collect();
    assert_eq!(sites.len(), 1, "{sites:?}");
    sites[0].verdict.clone()
}

fn proven(src: &str) -> bool {
    let v = verdict(src);
    assert!(
        !matches!(v, Verdict::No(ref s) if s.starts_with("dead site")),
        "{v:?}"
    );
    !matches!(v, Verdict::No(_))
}

fn program(bind: &str) -> String {
    format!(
        r#"
        class Inner {{ witness = 1; }}
        class Box {{ a = new Inner(); b: any = (globalThis as any).hostVal(); }}
        function read(box: any) {{
            {bind}
            const v = box[k];
            return v.witness;
        }}
        console.log(read(new Box()), read(new Box()));
    "#
    )
}

#[test]
fn const_key_names_its_field() {
    assert!(proven(&program(r#"const k = "a";"#)));
}

/// The literal it stands for: the same verdict.
#[test]
fn literal_key_names_its_field() {
    assert!(proven(
        &program(r#"const k = "a"; void k; const j = 0; void j;"#).replace("box[k]", "box[\"a\"]")
    ));
}

/// A binding that may be reassigned names no key: the read sees `b`, a
/// host value. (Before the program-prototype read took the key's entries,
/// `Box.prototype`'s `constructor` also failed this proof.)
#[test]
fn reassigned_key_names_none() {
    assert!(!proven(&program(
        r#"let k = "a"; if (box.b === "t") k = "b";"#
    )));
}

/// A key of the constants "a" and "b" reads the own fields `a` and `b`
/// and, on `Box.prototype`, only those names (ECMA-262 10.1.8.1, 7.3.2):
/// not its `constructor`. `b` is a String here, whose `witness` is absent.
#[test]
fn constant_key_set_reads_no_prototype_member() {
    assert!(proven(
        &program(r#"let k = "a"; if (box.b === "t") k = "b";"#)
            .replace("b: any = (globalThis as any).hostVal();", "b = \"s\";")
    ));
}

/// A const holding a non-literal names no key.
#[test]
fn const_of_unknown_names_none() {
    assert!(!proven(&program(r#"const k = box.b;"#)));
}
