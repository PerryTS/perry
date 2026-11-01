//! Native result proofs use operation identity and an admitted overload,
//! never a declared TypeScript return type or an assumed file content.
use super::*;

fn run(source: &str, units: bool) -> ProgramFacts {
    let parsed = perry_parser::parse_typescript(source, "t.ts").unwrap();
    let hir = perry_hir::lower_module(&parsed, "t", "t.ts").unwrap();
    let modules = [("t.ts".to_string(), &hir)];
    if units {
        crate::compute_units(&modules, &FactsConfig::default())
    } else {
        crate::compute(&modules, &FactsConfig::default())
    }
}

#[test]
fn successful_native_utf8_file_result_proves_string_length() {
    for units in [false, true] {
        for module in ["fs", "node:fs"] {
            for enc in ["utf8", "utf-8", "UTF-8"] {
                let pf = run(
                    &format!(
                        r#"
                    import {{ readFileSync as read }} from '{module}';
                    const data = read(process.argv[2], '{enc}');
                    console.log(data.length);
                "#
                    ),
                    units,
                );
                let site = pf.sites.iter().find(|s| s.key == "length").unwrap();
                assert_eq!(site.verdict, Verdict::OwnLength, "{}", pf.dump_text());
                assert!(!site.top);
                assert_eq!(site.receivers, ["primitive:string"]);
            }
        }
    }
}

#[test]
fn other_file_results_and_user_implementations_are_not_certified() {
    for units in [false, true] {
        for expression in [
            "read(process.argv[2])",
            "read(process.argv[2], null)",
            "read(process.argv[2], process.argv[3])",
            "read(process.argv[2], { encoding: 'utf8' })",
        ] {
            let pf = run(
                &format!(
                    r#"
                import {{ readFileSync as read }} from 'node:fs';
                const data: string = {expression}; console.log(data.length);
            "#
                ),
                units,
            );
            let site = pf.sites.iter().find(|s| s.key == "length").unwrap();
            assert!(matches!(site.verdict, Verdict::No(_)), "{}", pf.dump_text());
        }
        let pf = run(
            r#"
            const fs = { readFileSync(p: any, e: any) { return { length: 'wrong' }; } };
            const data = fs.readFileSync('a', 'utf8'); console.log(data.length);
        "#,
            units,
        );
        assert!(
            pf.sites
                .iter()
                .filter(|s| s.key == "length")
                .all(|s| s.verdict != Verdict::OwnLength),
            "{}",
            pf.dump_text()
        );
    }
}

#[test]
fn catch_result_does_not_inherit_the_successful_return_proof() {
    for units in [false, true] {
        let pf = run(
            r#"
            import { readFileSync as read } from 'node:fs';
            let data: any;
            try { data = read(process.argv[2], 'utf8'); }
            catch (error) { data = { length: false }; }
            console.log(data.length);
        "#,
            units,
        );
        assert!(
            pf.sites
                .iter()
                .filter(|s| s.key == "length")
                .all(|s| s.verdict != Verdict::OwnLength),
            "{}",
            pf.dump_text()
        );
    }
}
