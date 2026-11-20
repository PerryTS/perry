//! An Object.prototype method read as a value is that method: a call of it
//! runs its effect on the call's receiver (SOUND1 finding 3).
use super::*;

fn witness_proven(src: &str) -> bool {
    let pf = facts(src);
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "witness").collect();
    assert_eq!(sites.len(), 1, "{src}\n{}", pf.dump_text());
    let verdict = &sites[0].verdict;
    assert!(
        !matches!(verdict, Verdict::No(why) if why.starts_with("dead site")),
        "{src}: {verdict:?}",
    );
    !matches!(verdict, Verdict::No(_))
}

#[test]
fn annexb_method_value_installs_on_any_receiver() {
    let setup = r#"
        class Box { witness = 1; }
        const box: any = new Box();
        const plain: any = {};
        function cb() { return 2; }
    "#;
    for (method, proven) in [
        ("__defineGetter__", false),
        ("__defineSetter__", false),
        // A method that installs nothing: `box.witness` stays the field.
        ("toString", true),
    ] {
        let src = format!(
            "{setup}box.m = plain.{method};\nbox.m(\"witness\", cb);\nconsole.log(box.witness);"
        );
        assert_eq!(witness_proven(&src), proven, "{method}");
    }
}
