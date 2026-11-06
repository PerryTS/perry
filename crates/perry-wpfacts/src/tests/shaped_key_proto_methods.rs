//! A shaped computed key reaching `Object.prototype` reads every method its
//! shape can spell (SOUND1 finding 2).
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
fn shaped_key_reads_object_proto_methods_it_can_spell() {
    let setup = r#"
        class Cell { witness = 1; }
        let cell: any = new Cell();
        const t: any = {};
        const s: string = JSON.parse("\"Getter__\"");
    "#;
    // The getter replaces `cell` when `t.y` runs it.
    for (key, proven) in [
        // Spells `__defineGetter__`: the getter is installed and runs.
        ("\"__define\" + s", false),
        ("s + \"Getter__\"", false),
        // The named call, as before.
        ("\"__defineGetter__\"", false),
        // No Object.prototype method has this shape: the call throws.
        ("\"zz\" + s", true),
    ] {
        let src = format!(
            "{setup}try {{ t[{key}](\"y\", function () {{ cell = {{}}; return 0; }}); }} catch (e) {{}}\
             void t.y;\nconsole.log(cell.witness);"
        );
        assert_eq!(witness_proven(&src), proven, "{key}");
    }
}
