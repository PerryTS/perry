//! Annex B installs follow the single identity of aliased native functions.
use super::*;

#[test]
fn annexb_install_through_set_keys_is_read_through_values() {
    let setup = r#"
        declare function external(value: any): void;
        class Cell { witness = 1; }
        const o = new Cell();
        const s: any = new Set();
        function g() { external(o); }
    "#;
    for (install, read, proven) in [(false, true, true), (true, false, true), (true, true, false)] {
        let src = format!(
            "{setup}{}{}console.log(o.witness);",
            if install { "s.keys.__defineGetter__(\"zap\", () => g);" } else { "" },
            if read { "s.values.zap(o);" } else { "" },
        );
        let pf = facts(&src);
        let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "witness").collect();
        assert_eq!(sites.len(), 1, "{src}\n{}", pf.dump_text());
        let verdict = &sites[0].verdict;
        assert!(
            !matches!(verdict, Verdict::No(why) if why.starts_with("dead site")),
            "install={install} read={read}: {verdict:?}",
        );
        assert_eq!(
            !matches!(verdict, Verdict::No(_)), proven,
            "install={install} read={read}: {verdict:?}\n{}", pf.dump_text(),
        );
    }
}
