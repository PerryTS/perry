//! `Expr::EnumMember` is its member's value: a number member is a number, a
//! string member a string. As an array index the number is an element read;
//! the string names a key (here an intrinsic method), whose value is a builtin
//! function that ToPrimitive of the holder then calls.
use super::*;

fn holder_escape(index: &str, decl: &str) -> Option<String> {
    let pf = facts(&format!(
        "{decl}
         class Holder {{ x = 1; }}
         const names: any[] = [];
         names[0] = \"zero\"; names[1] = \"one\";
         function nameOf(k: any): any {{ return names[k]; }}
         const holder: any = new Holder();
         const key: string = process.argv[2] ?? \"k\";
         holder[key] = nameOf({index});
         console.log(\"holder=\" + holder);"
    ));
    tmpl(&pf, "Holder").escape.clone()
}

#[test]
fn numeric_member_index_is_an_element_read() {
    let e = holder_escape("Kind.One", "enum Kind { Zero, One, Two }");
    assert_eq!(e, None, "numeric enum member escaped the holder");
}

#[test]
fn literal_index_control_is_an_element_read() {
    assert_eq!(holder_escape("1", "enum Kind { Zero, One, Two }"), None);
}

#[test]
fn string_member_index_is_still_a_string_key() {
    let e = holder_escape("S.A", "enum S { A = \"push\" }");
    assert!(e.is_some(), "a string enum member must stay a string key");
}
